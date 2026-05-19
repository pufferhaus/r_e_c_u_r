//! Integration test: file + capture slots round-trip through banks.toml.

use recur::persist;
use recur::state::{Bank, Slot, SourceKind};

#[test]
fn new_form_with_capture_source_round_trips() {
    use recur::capture::CaptureDevice;
    let tmp = tempfile::tempdir().unwrap();
    let mut b = Bank::empty();
    b.slots[0] = Some(Slot {
        source: SourceKind::Capture(CaptureDevice {
            path: "/dev/video0".into(),
            label: "v4l2:video0".into(),
        }),
        name: "v4l2:video0".into(),
        start: -1.0,
        end: -1.0,
        length: 0.0,
        rate: 1.0,
    });
    persist::save_banks(tmp.path(), &[b.clone()]).unwrap();
    let got = persist::load_banks(tmp.path()).unwrap();
    assert_eq!(got, vec![b]);
}

#[test]
fn mixed_file_and_capture_slots_round_trip() {
    use recur::capture::CaptureDevice;
    let tmp = tempfile::tempdir().unwrap();
    let mut b = Bank::empty();
    b.slots[0] = Some(Slot {
        source: SourceKind::File("/clips/a.mp4".into()),
        name: "a.mp4".into(),
        start: 1.5,
        end: 4.2,
        length: 10.0,
        rate: 1.0,
    });
    b.slots[3] = Some(Slot {
        source: SourceKind::Capture(CaptureDevice {
            path: "/dev/video1".into(),
            label: "v4l2:video1".into(),
        }),
        name: "v4l2:video1".into(),
        start: -1.0,
        end: -1.0,
        length: 0.0,
        rate: 1.0,
    });
    persist::save_banks(tmp.path(), &[b.clone()]).unwrap();
    let got = persist::load_banks(tmp.path()).unwrap();
    assert_eq!(got, vec![b]);
}
