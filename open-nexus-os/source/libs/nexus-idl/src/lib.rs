#![cfg_attr(not(test), no_std)]

/// Declarative macro for defining service interfaces and exposing descriptors.
#[macro_export]
macro_rules! nexus_interface {
    (interface $iface:ident { $(fn $method:ident(&self $(, $arg:ident : $ty:ty)*) -> $ret:ty;)* }) => {
        pub mod $iface {
            pub trait Service {
                $(fn $method(&self $(, $arg : $ty)*) -> $ret;)*
            }

            pub fn descriptor() -> &'static [&'static str] {
                &[$(stringify!($method)),*]
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::nexus_interface;

    nexus_interface!(interface testsvc {
        fn ping(&self, token: u32) -> u32;
        fn shutdown(&self) -> ();
    });

    struct Stub;

    impl testsvc::Service for Stub {
        fn ping(&self, token: u32) -> u32 {
            token
        }

        fn shutdown(&self) -> () {
            ()
        }
    }

    #[test]
    fn descriptor_lists_methods() {
        let names = testsvc::descriptor();
        assert_eq!(names, ["ping", "shutdown"]);
    }
}
