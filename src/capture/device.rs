//! Live capture devices (v4l2 on Linux, avfvideosrc on macOS).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptureDevice {
    /// GStreamer source argument: `/dev/videoN` on Linux, a numeric index on macOS.
    pub path: String,
    /// Display label for the SamplerBody annotation.
    pub label: String,
}

/// Enumerate available capture devices on the current OS. May return empty.
pub fn enumerate_capture_devices() -> Vec<CaptureDevice> {
    #[cfg(target_os = "linux")]
    {
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir("/dev") else {
            return out;
        };
        for e in entries.flatten() {
            let p = e.path();
            let Some(name) = p.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if let Some(suffix) = name.strip_prefix("video") {
                if !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()) {
                    out.push(CaptureDevice {
                        path: p.display().to_string(),
                        label: format!("v4l2:{name}"),
                    });
                }
            }
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        out
    }

    #[cfg(target_os = "macos")]
    {
        vec![CaptureDevice {
            path: "0".to_string(),
            label: "avf-camera-0".to_string(),
        }]
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_device_round_trips_through_toml() {
        let d = CaptureDevice {
            path: "/dev/video0".into(),
            label: "v4l2:video0".into(),
        };
        let s = toml::to_string(&d).unwrap();
        let back: CaptureDevice = toml::from_str(&s).unwrap();
        assert_eq!(d, back);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn enumerate_returns_at_least_one_on_macos() {
        let devs = enumerate_capture_devices();
        assert!(!devs.is_empty(), "macos always returns at least one");
        assert_eq!(devs[0].path, "0");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn enumerate_does_not_panic_on_linux() {
        let _ = enumerate_capture_devices();
    }
}
