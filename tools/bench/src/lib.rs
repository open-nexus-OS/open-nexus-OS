//! CONTEXT: Benchmark library
//! INTENT: Performance measurement utilities
//! IDL (target): spin(count), measure(func), report()
//! DEPS: std::time (timing)
//! READINESS: Library; no service dependencies
//! TESTS: Spin runs; measurement accuracy
pub fn spin(count: u64) -> u64 {
    let mut acc: u64 = 0;
    for i in 0..count {
        acc = acc.wrapping_add(i);
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::spin;

    #[test]
    fn spin_runs() {
        assert_eq!(spin(3), 3);
    }
}
