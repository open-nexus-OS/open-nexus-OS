pub fn spin(count: u64) -> u64 {
    let mut acc = 0;
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
