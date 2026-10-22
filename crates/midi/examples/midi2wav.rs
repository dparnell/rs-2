//! Render a MIDI file to a WAV file with rs2's synthesizer, for listening:
//! `cargo run -p midi --example midi2wav IN.MID OUT.WAV [seconds]`.

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let data = std::fs::read(&a[1]).expect("read");
    let song = midi::parse(&data).expect("MIDI");
    let secs: u64 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(song.length_us / 1_000_000 + 2);
    let rate = 44100u32;
    let mut p = midi::Player::new(song, rate);
    let mut pcm = vec![0i16; (rate as u64 * secs * 2) as usize];
    p.render(&mut pcm);
    let bytes: Vec<u8> = pcm.iter().flat_map(|v| v.to_le_bytes()).collect();
    let mut w = Vec::new();
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + bytes.len() as u32).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&[1, 0, 2, 0]);
    w.extend_from_slice(&rate.to_le_bytes());
    w.extend_from_slice(&(rate * 4).to_le_bytes());
    w.extend_from_slice(&[4, 0, 16, 0]);
    w.extend_from_slice(b"data");
    w.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    w.extend_from_slice(&bytes);
    std::fs::write(&a[2], w).expect("write");
}
