use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct SpeedSample {
    pub speed: f64,
    pub progress: f64,
}

pub async fn run_speed_test<F>(mut on_progress: F) -> Result<f64, String>
where
    F: FnMut(SpeedSample) + Send,
{
    use futures_util::StreamExt;
    use std::time::Instant;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36")
        .build()
        .map_err(|e| e.to_string())?;

    let test_urls = [
        "https://speed.cloudflare.com/__down?bytes=25000000",
        "https://proof.ovh.net/files/100Mb.dat",
    ];

    for test_url in test_urls {
        match client.get(test_url).send().await {
            Ok(response) => {
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
                                let progress = ((elapsed / max_duration) * 100.0).min(100.0);
                                on_progress(SpeedSample {
                                    speed: mbps,
                                    progress,
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
                    progress: 100.0,
                });
                return Ok((mbps * 100.0).round() / 100.0);
            }
            Err(_) => continue,
        }
    }

    Err("All speed test servers failed. Please check your internet connection.".to_string())
}
