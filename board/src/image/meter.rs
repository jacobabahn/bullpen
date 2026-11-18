//! Where the image look's time and bytes go, for its benchmark (tests/test_bench.rs):
//! the parts of drawing and sending a frame (drawing an image, its PNG, base64, the
//! writing) note how long they took and what they wrote, while a test has the meter
//! on. In the board itself it notes nothing: each note is the work alone.

/// What was noted: milliseconds and bytes, by part ("png Sel", "write").
#[cfg(test)]
#[derive(Debug, Clone, Default)]
pub struct Tally {
    pub ms: std::collections::BTreeMap<String, f64>,
    pub bytes: std::collections::BTreeMap<String, usize>,
}

#[cfg(test)]
thread_local! {
    static ON: std::cell::RefCell<Option<Tally>> = const { std::cell::RefCell::new(None) };
}

/// Do `f`, noting how long it took as `part`.
pub fn time<T>(part: impl FnOnce() -> String, f: impl FnOnce() -> T) -> T {
    #[cfg(test)]
    if ON.with(|on| on.borrow().is_some()) {
        let t = std::time::Instant::now();
        let out = f();
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        ON.with(|on| on.borrow_mut().as_mut().map(|tally| *tally.ms.entry(part()).or_default() += ms));
        return out;
    }
    #[cfg(not(test))]
    let _ = part;
    f()
}

/// Note `n` bytes written as `part`.
pub fn bytes(part: impl FnOnce() -> String, n: usize) {
    #[cfg(test)]
    ON.with(|on| on.borrow_mut().as_mut().map(|tally| *tally.bytes.entry(part()).or_default() += n));
    #[cfg(not(test))]
    let _ = (part, n);
}

/// Start noting, from nothing.
#[cfg(test)]
pub fn start() {
    ON.with(|on| *on.borrow_mut() = Some(Tally::default()));
}

/// What was noted since `start`, and start again.
#[cfg(test)]
pub fn take() -> Tally {
    ON.with(|on| on.borrow_mut().replace(Tally::default()).unwrap_or_default())
}

/// Stop noting.
#[cfg(test)]
pub fn stop() {
    ON.with(|on| *on.borrow_mut() = None);
}
