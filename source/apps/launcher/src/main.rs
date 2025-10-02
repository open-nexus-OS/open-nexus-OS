fn main() {
    println!("Launcher started");
}

#[cfg(test)]
mod tests {
    #[test]
    fn message_constant() {
        assert_eq!("Launcher started", "Launcher started");
    }
}
