//! End-to-end ring push + get round-trip.

use recur::detour::Ring;

#[test]
fn ring_push_100_frames_at_320x240_round_trips() {
    let bytes_per_frame = 320 * 240 * 4;
    let mut r = Ring::new(320, 240, 100 * bytes_per_frame);
    assert_eq!(r.capacity(), 100);

    for i in 0..100 {
        let frame: Vec<u8> = (0..bytes_per_frame).map(|_| i as u8).collect();
        r.push(&frame);
    }
    assert_eq!(r.count(), 100);
    assert_eq!(r.get(0).unwrap()[0], 0);
    assert_eq!(r.get(99).unwrap()[0], 99);

    let frame: Vec<u8> = vec![200; bytes_per_frame];
    r.push(&frame);
    assert_eq!(r.count(), 100);
    assert_eq!(r.get(0).unwrap()[0], 1);
    assert_eq!(r.get(99).unwrap()[0], 200);
}
