//! Drop-a-file-get-a-link backends. 0x0.st is always available; WebDAV and
//! S3 need credentials from [`crate::share::ShareSettings`].

use super::{LinkBackendKind, ShareSettings};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const ZEROX_URL: &str = "https://0x0.st";
const ZEROX_MAX_BYTES: u64 = 512 * 1024 * 1024;
const ZEROX_MIN_AGE_DAYS: f64 = 30.0;
const ZEROX_MAX_AGE_DAYS: f64 = 365.0;

pub trait LinkBackend {
    fn upload(&self, path: &Path) -> impl std::future::Future<Output = Result<UploadResult, String>> + Send;
}

#[derive(Debug, Clone, PartialEq)]
pub struct UploadResult {
    pub url: String,
    pub delete_token: Option<String>,
    pub retention_days: Option<f64>,
}

pub struct ZeroXZero;
pub struct WebDav {
    pub base_url: String,
    pub username: String,
    pub password: String,
}
pub struct S3 {
    pub bucket: String,
    pub region: String,
    pub endpoint: String,
    pub access_key: String,
    pub secret_key: String,
    pub public_base: String,
}

pub enum LinkUpload {
    ZeroXZero(ZeroXZero),
    WebDav(WebDav),
    S3(S3),
}

impl LinkUpload {
    pub fn from_settings(settings: &ShareSettings) -> Result<Self, String> {
        match settings.link_backend {
            LinkBackendKind::ZeroXZero => Ok(Self::ZeroXZero(ZeroXZero)),
            LinkBackendKind::WebDav => {
                if settings.webdav_url.trim().is_empty() {
                    return Err("set a WebDAV base URL in Settings".into());
                }
                Ok(Self::WebDav(WebDav {
                    base_url: settings.webdav_url.clone(),
                    username: settings.webdav_username.clone(),
                    password: settings.webdav_password.clone(),
                }))
            }
            LinkBackendKind::S3 => {
                if settings.s3_bucket.trim().is_empty() || settings.s3_access_key.trim().is_empty()
                {
                    return Err("set an S3 bucket and access key in Settings".into());
                }
                if settings.s3_secret_key.trim().is_empty() {
                    return Err("set the S3 secret key in Settings".into());
                }
                Ok(Self::S3(S3 {
                    bucket: settings.s3_bucket.clone(),
                    region: if settings.s3_region.trim().is_empty() {
                        "us-east-1".into()
                    } else {
                        settings.s3_region.clone()
                    },
                    endpoint: settings.s3_endpoint.clone(),
                    access_key: settings.s3_access_key.clone(),
                    secret_key: settings.s3_secret_key.clone(),
                    public_base: settings.s3_public_base.clone(),
                }))
            }
        }
    }

    pub async fn upload(&self, path: &Path) -> Result<UploadResult, String> {
        match self {
            Self::ZeroXZero(backend) => backend.upload(path).await,
            Self::WebDav(backend) => backend.upload(path).await,
            Self::S3(backend) => backend.upload(path).await,
        }
    }
}

/// 0x0.st retention: `min + (-max + min) * (size/max_size - 1)^3`.
pub fn zero_x_zero_retention_days(file_size: u64) -> f64 {
    let ratio = (file_size as f64 / ZEROX_MAX_BYTES as f64) - 1.0;
    ZEROX_MIN_AGE_DAYS + (-ZEROX_MAX_AGE_DAYS + ZEROX_MIN_AGE_DAYS) * ratio.powi(3)
}

pub fn parse_zero_x_zero_body(body: &str) -> Result<String, String> {
    let url = body.trim();
    if url.starts_with("http://") || url.starts_with("https://") {
        Ok(url.to_string())
    } else {
        Err(format!("0x0.st did not return a URL: {url}"))
    }
}

impl LinkBackend for ZeroXZero {
    async fn upload(&self, path: &Path) -> Result<UploadResult, String> {
        let meta = std::fs::metadata(path).map_err(|err| err.to_string())?;
        if meta.len() > ZEROX_MAX_BYTES {
            return Err("0x0.st rejects files larger than 512 MiB".into());
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".into());
        let bytes = tokio::fs::read(path).await.map_err(|err| err.to_string())?;
        let part = reqwest::multipart::Part::bytes(bytes)
            .file_name(name)
            .mime_str(&super::localsend::mime_for_path(&path.to_string_lossy()))
            .map_err(|err| err.to_string())?;
        let form = reqwest::multipart::Form::new().part("file", part);
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|err| err.to_string())?;
        let response = client
            .post(ZEROX_URL)
            .multipart(form)
            .send()
            .await
            .map_err(|err| err.to_string())?;
        if !response.status().is_success() {
            return Err(format!("0x0.st rejected the upload ({})", response.status()));
        }
        let token = response
            .headers()
            .get("x-token")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let body = response.text().await.map_err(|err| err.to_string())?;
        Ok(UploadResult {
            url: parse_zero_x_zero_body(&body)?,
            delete_token: token,
            retention_days: Some(zero_x_zero_retention_days(meta.len())),
        })
    }
}

pub fn join_webdav_url(base: &str, filename: &str) -> Result<String, String> {
    let base = base.trim().trim_end_matches('/');
    if !(base.starts_with("http://") || base.starts_with("https://")) {
        return Err("WebDAV URL must start with http:// or https://".into());
    }
    let name = filename.rsplit('/').next().unwrap_or(filename);
    if name.is_empty() || name == "." || name == ".." {
        return Err("invalid file name".into());
    }
    Ok(format!("{base}/{}", super::localsend::urlencode(name)))
}

impl LinkBackend for WebDav {
    async fn upload(&self, path: &Path) -> Result<UploadResult, String> {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".into());
        let url = join_webdav_url(&self.base_url, &name)?;
        let bytes = tokio::fs::read(path).await.map_err(|err| err.to_string())?;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|err| err.to_string())?;
        let mut request = client.put(&url).body(bytes);
        if !self.username.is_empty() || !self.password.is_empty() {
            request = request.basic_auth(&self.username, Some(&self.password));
        }
        let response = request.send().await.map_err(|err| err.to_string())?;
        if !response.status().is_success() {
            return Err(format!("WebDAV PUT failed ({})", response.status()));
        }
        Ok(UploadResult {
            url,
            delete_token: None,
            retention_days: None,
        })
    }
}

pub fn uri_encode(value: &str, encode_slash: bool) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            b'/' if !encode_slash => out.push('/'),
            _ => {
                out.push('%');
                out.push_str(&super::localsend::hex_encode(&[byte]).to_ascii_uppercase());
            }
        }
    }
    out
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("hmac key");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn sha256_hex(bytes: &[u8]) -> String {
    super::localsend::hex_encode(&Sha256::digest(bytes))
}

/// AWS SigV4 signing key: HMAC chain over date, region, service, request.
pub fn aws_signing_key(secret: &str, datestamp: &str, region: &str, service: &str) -> Vec<u8> {
    let k_date = hmac_sha256(format!("AWS4{secret}").as_bytes(), datestamp.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, service.as_bytes());
    hmac_sha256(&k_service, b"aws4_request")
}

pub fn aws_canonical_request(
    method: &str,
    canonical_uri: &str,
    canonical_query: &str,
    canonical_headers: &str,
    signed_headers: &str,
    payload_hash: &str,
) -> String {
    format!(
        "{method}\n{canonical_uri}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\n{payload_hash}"
    )
}

pub fn aws_string_to_sign(algorithm: &str, amz_date: &str, scope: &str, canonical: &str) -> String {
    format!("{algorithm}\n{amz_date}\n{scope}\n{}", sha256_hex(canonical.as_bytes()))
}

pub fn aws_authorization(
    access_key: &str,
    scope: &str,
    signed_headers: &str,
    signature: &str,
) -> String {
    format!(
        "AWS4-HMAC-SHA256 Credential={access_key}/{scope}, SignedHeaders={signed_headers}, Signature={signature}"
    )
}

pub fn s3_host(bucket: &str, region: &str, endpoint: &str) -> String {
    let endpoint = endpoint.trim().trim_end_matches('/');
    if !endpoint.is_empty() {
        let host = endpoint
            .strip_prefix("https://")
            .or_else(|| endpoint.strip_prefix("http://"))
            .unwrap_or(endpoint);
        if host.starts_with(bucket) {
            host.to_string()
        } else {
            format!("{bucket}.{host}")
        }
    } else if region == "us-east-1" {
        format!("{bucket}.s3.amazonaws.com")
    } else {
        format!("{bucket}.s3.{region}.amazonaws.com")
    }
}

pub fn s3_public_url(settings: &S3, key: &str) -> String {
    let base = settings.public_base.trim().trim_end_matches('/');
    if !base.is_empty() {
        return format!("{base}/{}", uri_encode(key, false));
    }
    format!("https://{}/{}", s3_host(&settings.bucket, &settings.region, &settings.endpoint), uri_encode(key, false))
}

fn amz_timestamps(now: SystemTime) -> Result<(String, String), String> {
    let secs = now
        .duration_since(UNIX_EPOCH)
        .map_err(|err| err.to_string())?
        .as_secs();
    // Civil date from Unix seconds (UTC). Good enough for SigV4 timestamps.
    let days = secs / 86400;
    let tod = secs % 86400;
    let (year, month, day) = civil_from_days(days as i64);
    let hour = tod / 3600;
    let min = (tod % 3600) / 60;
    let sec = tod % 60;
    let date = format!("{year:04}{month:02}{day:02}");
    let amz = format!("{date}T{hour:02}{min:02}{sec:02}Z");
    Ok((date, amz))
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    // Howard Hinnant civil-from-days (proleptic Gregorian).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year as i32, m as u32, d as u32)
}

impl LinkBackend for S3 {
    async fn upload(&self, path: &Path) -> Result<UploadResult, String> {
        let key = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".into());
        let payload = tokio::fs::read(path).await.map_err(|err| err.to_string())?;
        let payload_hash = sha256_hex(&payload);
        let host = s3_host(&self.bucket, &self.region, &self.endpoint);
        let canonical_uri = format!("/{}", uri_encode(&key, true));
        let (datestamp, amz_date) = amz_timestamps(SystemTime::now())?;
        let scope = format!("{datestamp}/{}/s3/aws4_request", self.region);
        let canonical_headers = format!(
            "host:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\n"
        );
        let signed_headers = "host;x-amz-content-sha256;x-amz-date";
        let canonical = aws_canonical_request(
            "PUT",
            &canonical_uri,
            "",
            &canonical_headers,
            signed_headers,
            &payload_hash,
        );
        let to_sign = aws_string_to_sign("AWS4-HMAC-SHA256", &amz_date, &scope, &canonical);
        let signature = super::localsend::hex_encode(&hmac_sha256(
            &aws_signing_key(&self.secret_key, &datestamp, &self.region, "s3"),
            to_sign.as_bytes(),
        ));
        let authorization = aws_authorization(&self.access_key, &scope, signed_headers, &signature);
        let scheme = if self.endpoint.trim().starts_with("http://") {
            "http"
        } else {
            "https"
        };
        let url = format!("{scheme}://{host}{canonical_uri}");
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|err| err.to_string())?;
        let response = client
            .put(&url)
            .header("host", &host)
            .header("x-amz-content-sha256", &payload_hash)
            .header("x-amz-date", &amz_date)
            .header(reqwest::header::AUTHORIZATION, authorization)
            .header(
                reqwest::header::CONTENT_TYPE,
                super::localsend::mime_for_path(&path.to_string_lossy()),
            )
            .body(payload)
            .send()
            .await
            .map_err(|err| err.to_string())?;
        if !response.status().is_success() {
            return Err(format!("S3 PUT failed ({})", response.status()));
        }
        Ok(UploadResult {
            url: s3_public_url(self, &key),
            delete_token: None,
            retention_days: if self.public_base.trim().is_empty() {
                Some(7.0)
            } else {
                None
            },
        })
    }
}

pub async fn upload_path(settings: &ShareSettings, path: &Path) -> Result<UploadResult, String> {
    LinkUpload::from_settings(settings)?.upload(path).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_x_zero_retention_is_365_for_tiny_and_30_for_cap() {
        let tiny = zero_x_zero_retention_days(0);
        let full = zero_x_zero_retention_days(ZEROX_MAX_BYTES);
        assert!((tiny - 365.0).abs() < 0.01, "tiny={tiny}");
        assert!((full - 30.0).abs() < 0.01, "full={full}");
        let mid = zero_x_zero_retention_days(ZEROX_MAX_BYTES / 2);
        assert!(mid > 30.0 && mid < 365.0);
    }

    #[test]
    fn parse_zero_x_zero_body_requires_url() {
        assert_eq!(
            parse_zero_x_zero_body("https://0x0.st/abcd.png\n").unwrap(),
            "https://0x0.st/abcd.png"
        );
        assert!(parse_zero_x_zero_body("rate limited").is_err());
    }

    #[test]
    fn join_webdav_encodes_the_file_name() {
        assert_eq!(
            join_webdav_url("https://dav.example/public/", "my file.txt").unwrap(),
            "https://dav.example/public/my%20file.txt"
        );
        assert!(join_webdav_url("dav.example", "a").is_err());
        assert!(join_webdav_url("https://dav.example", "..").is_err());
    }

    #[test]
    fn s3_host_virtual_and_custom_endpoint() {
        assert_eq!(
            s3_host("bucket", "us-east-1", ""),
            "bucket.s3.amazonaws.com"
        );
        assert_eq!(
            s3_host("bucket", "eu-west-1", ""),
            "bucket.s3.eu-west-1.amazonaws.com"
        );
        assert_eq!(
            s3_host("bucket", "us-east-1", "https://minio.local"),
            "bucket.minio.local"
        );
    }

    #[test]
    fn aws_canonical_and_string_to_sign_match_known_shape() {
        let canonical = aws_canonical_request(
            "GET",
            "/test.txt",
            "",
            "host:examplebucket.s3.amazonaws.com\nrange:bytes=0-9\nx-amz-content-sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\nx-amz-date:20130524T000000Z\n",
            "host;range;x-amz-content-sha256;x-amz-date",
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        );
        assert!(canonical.starts_with("GET\n/test.txt\n\n"));
        let scope = "20130524/us-east-1/s3/aws4_request";
        let to_sign = aws_string_to_sign("AWS4-HMAC-SHA256", "20130524T000000Z", scope, &canonical);
        assert!(to_sign.starts_with("AWS4-HMAC-SHA256\n20130524T000000Z\n20130524/us-east-1/s3/aws4_request\n"));
        assert_eq!(to_sign.lines().count(), 4);
        let key = aws_signing_key("wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY", "20130524", "us-east-1", "s3");
        assert_eq!(key.len(), 32);
        let auth = aws_authorization("AKIA", scope, "host", "abc");
        assert!(auth.contains("Credential=AKIA/20130524/us-east-1/s3/aws4_request"));
        assert!(auth.contains("Signature=abc"));
    }

    #[test]
    fn public_url_prefers_configured_base() {
        let s3 = S3 {
            bucket: "b".into(),
            region: "us-east-1".into(),
            endpoint: String::new(),
            access_key: "a".into(),
            secret_key: "s".into(),
            public_base: "https://cdn.example".into(),
        };
        assert_eq!(s3_public_url(&s3, "x y"), "https://cdn.example/x%20y");
        let unset = S3 {
            public_base: String::new(),
            ..s3
        };
        assert_eq!(
            s3_public_url(&unset, "x"),
            "https://b.s3.amazonaws.com/x"
        );
    }

    #[test]
    fn from_settings_requires_credentials() {
        let mut settings = ShareSettings::default();
        assert!(matches!(
            LinkUpload::from_settings(&settings),
            Ok(LinkUpload::ZeroXZero(_))
        ));
        settings.link_backend = LinkBackendKind::WebDav;
        assert!(LinkUpload::from_settings(&settings).is_err());
        settings.webdav_url = "https://dav.example/public".into();
        assert!(matches!(
            LinkUpload::from_settings(&settings),
            Ok(LinkUpload::WebDav(_))
        ));
        settings.link_backend = LinkBackendKind::S3;
        assert!(LinkUpload::from_settings(&settings).is_err());
        settings.s3_bucket = "b".into();
        settings.s3_access_key = "a".into();
        settings.s3_secret_key = "s".into();
        assert!(matches!(
            LinkUpload::from_settings(&settings),
            Ok(LinkUpload::S3(_))
        ));
    }

    #[test]
    fn civil_from_unix_epoch_and_aws_example_date() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        // 2013-05-24 is the AWS SigV4 example date (unix 1369353600).
        assert_eq!(civil_from_days(1369353600 / 86400), (2013, 5, 24));
    }
}
