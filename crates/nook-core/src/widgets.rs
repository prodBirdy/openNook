use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum SpeedPhase {
    Download,
    Upload,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct SpeedSample {
    pub speed: f64,
    pub progress: f64,
    pub phase: SpeedPhase,
}

#[derive(Clone, Debug, Serialize)]
pub struct SpeedResult {
    pub down_mbps: f64,
    pub up_mbps: Option<f64>,
}

pub async fn run_speed_test<F>(mut on_progress: F) -> Result<SpeedResult, String>
where
    F: FnMut(SpeedSample) + Send,
{
    use futures_util::StreamExt;
    use std::time::Instant;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent("openNook (https://github.com/prodBirdy/openNook)")
        .build()
        .map_err(|e| e.to_string())?;

    let test_urls = [
        "https://speed.cloudflare.com/__down?bytes=25000000",
        "https://proof.ovh.net/files/100Mb.dat",
    ];

    for test_url in test_urls {
        match client.get(test_url).send().await {
            Ok(response) => {
                let expected = response
                    .content_length()
                    .filter(|&n| n > 0)
                    .unwrap_or(25_000_000);
                let mut stream = response.bytes_stream();
                let start = Instant::now();
                let mut total_bytes = 0u64;
                let mut last_sample_time = start;
                let mut sent_first = false;
                let max_duration = 8.0;

                while let Some(chunk_result) = stream.next().await {
                    match chunk_result {
                        Ok(chunk) => {
                            total_bytes += chunk.len() as u64;
                            let due = !sent_first || last_sample_time.elapsed().as_millis() >= 100;
                            if total_bytes > 0 && due {
                                let elapsed = start.elapsed().as_secs_f64().max(1e-6);
                                let bps = (total_bytes as f64 * 8.0) / elapsed;
                                let mbps = bps / 1_000_000.0;
                                last_sample_time = Instant::now();
                                sent_first = true;
                                let by_time = elapsed / max_duration;
                                let by_bytes = total_bytes as f64 / expected as f64;
                                let progress = (by_time.max(by_bytes) * 50.0).min(50.0);
                                on_progress(SpeedSample {
                                    speed: mbps,
                                    progress,
                                    phase: SpeedPhase::Download,
                                });
                            }
                            if start.elapsed().as_secs() >= 8 {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }

                let elapsed = start.elapsed().as_secs_f64();
                if elapsed < 1.0 || total_bytes < 1024 * 1024 {
                    continue;
                }
                let bps = (total_bytes as f64 * 8.0) / elapsed;
                let mbps = bps / 1_000_000.0;
                on_progress(SpeedSample {
                    speed: mbps,
                    progress: 50.0,
                    phase: SpeedPhase::Download,
                });
                let down_mbps = (mbps * 100.0).round() / 100.0;
                let up_mbps = upload_phase(&client, &mut on_progress).await;
                return Ok(SpeedResult { down_mbps, up_mbps });
            }
            Err(_) => continue,
        }
    }

    Err("All speed test servers failed. Please check your internet connection.".to_string())
}

/// Upload phase: 10 MB streamed in 64 KiB chunks, timed from the first chunk to
/// the response. A failure yields `None` so the download figure still lands.
async fn upload_phase<F>(client: &reqwest::Client, on_progress: &mut F) -> Option<f64>
where
    F: FnMut(SpeedSample) + Send,
{
    use futures_util::future::{self, Either};

    const TOTAL: u64 = 10 * 1024 * 1024;
    const CHUNK: u64 = 64 * 1024;
    const CAP_SECS: f64 = 8.0;

    let started = std::time::Instant::now();
    let sent = Arc::new(AtomicU64::new(0));
    let counter = sent.clone();
    let body = futures_util::stream::unfold(0u64, move |offset| {
        let counter = counter.clone();
        async move {
            if offset >= TOTAL || started.elapsed().as_secs_f64() >= CAP_SECS {
                return None;
            }
            let len = (TOTAL - offset).min(CHUNK) as usize;
            counter.fetch_add(len as u64, Ordering::Relaxed);
            Some((
                Ok::<Vec<u8>, std::io::Error>(vec![0u8; len]),
                offset + len as u64,
            ))
        }
    });

    let request = client
        .post("https://speed.cloudflare.com/__up")
        .body(reqwest::Body::wrap_stream(body))
        .send();
    futures_util::pin_mut!(request);

    let response = loop {
        let tick = tokio::time::sleep(std::time::Duration::from_millis(100));
        futures_util::pin_mut!(tick);
        match future::select(request.as_mut(), tick).await {
            Either::Left((res, _)) => break res,
            Either::Right((_, _)) => {
                let bytes = sent.load(Ordering::Relaxed);
                let progress = (50.0 + 50.0 * bytes as f64 / TOTAL as f64).min(100.0);
                on_progress(SpeedSample {
                    speed: upload_mbps(bytes, started),
                    progress,
                    phase: SpeedPhase::Upload,
                });
            }
        }
    };

    response.ok()?;
    let bytes = sent.load(Ordering::Relaxed);
    on_progress(SpeedSample {
        speed: upload_mbps(bytes, started),
        progress: 100.0,
        phase: SpeedPhase::Upload,
    });
    Some((upload_mbps(bytes, started) * 100.0).round() / 100.0)
}

fn upload_mbps(bytes: u64, started: std::time::Instant) -> f64 {
    let elapsed = started.elapsed().as_secs_f64().max(1e-6);
    (bytes as f64 * 8.0) / elapsed / 1_000_000.0
}
