// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use capnp::private::layout::{
    PointerBuilder, PointerReader, StructBuilder, StructReader, StructSize,
};
use capnp::traits::{FromPointerBuilder, FromPointerReader};
use capnp::{Result, Word};

const INSTALL_REQUEST_SIZE: StructSize = StructSize {
    data: 1,
    pointers: 1,
};
const INSTALL_RESPONSE_SIZE: StructSize = StructSize {
    data: 1,
    pointers: 0,
};
const QUERY_REQUEST_SIZE: StructSize = StructSize {
    data: 0,
    pointers: 1,
};
const QUERY_RESPONSE_SIZE: StructSize = StructSize {
    data: 1,
    pointers: 1,
};

pub mod install_error {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Type {
        None = 0,
        Eacces = 1,
        Einval = 2,
        Ebusy = 3,
        Enoent = 4,
    }

    impl Type {
        pub fn from_u16(value: u16) -> Option<Self> {
            match value {
                0 => Some(Self::None),
                1 => Some(Self::Eacces),
                2 => Some(Self::Einval),
                3 => Some(Self::Ebusy),
                4 => Some(Self::Enoent),
                _ => None,
            }
        }
    }
}

pub mod install_request {
    use super::*;

    #[derive(Clone, Copy)]
    pub struct Reader<'a> {
        reader: StructReader<'a>,
    }

    impl<'a> Reader<'a> {
        pub fn get_name(&self) -> Result<&'a str> {
            self.reader
                .get_pointer_field(0)
                .get_text(None)?
                .to_str()
                .map_err(|err| capnp::Error::failed(err.to_string()))
        }

        pub fn get_bytes_len(&self) -> u32 {
            self.reader.get_data_field::<u32>(0)
        }

        pub fn get_vmo_handle(&self) -> u32 {
            self.reader.get_data_field::<u32>(1)
        }
    }

    impl<'a> From<StructReader<'a>> for Reader<'a> {
        fn from(reader: StructReader<'a>) -> Self {
            Self { reader }
        }
    }

    impl<'a> FromPointerReader<'a> for Reader<'a> {
        fn get_from_pointer(
            reader: &PointerReader<'a>,
            default: Option<&'a [Word]>,
        ) -> Result<Self> {
            reader.get_struct(default).map(Self::from)
        }
    }

    pub struct Builder<'a> {
        builder: StructBuilder<'a>,
    }

    impl<'a> Builder<'a> {
        pub fn set_name(&mut self, value: &str) {
            self.builder
                .reborrow()
                .get_pointer_field(0)
                .set_text(value.into());
        }

        pub fn set_bytes_len(&mut self, value: u32) {
            self.builder.set_data_field::<u32>(0, value);
        }

        pub fn set_vmo_handle(&mut self, value: u32) {
            self.builder.set_data_field::<u32>(1, value);
        }
    }

    impl<'a> From<StructBuilder<'a>> for Builder<'a> {
        fn from(builder: StructBuilder<'a>) -> Self {
            Self { builder }
        }
    }

    impl<'a> FromPointerBuilder<'a> for Builder<'a> {
        fn init_pointer(builder: PointerBuilder<'a>, _size: u32) -> Self {
            builder.init_struct(INSTALL_REQUEST_SIZE).into()
        }

        fn get_from_pointer(
            builder: PointerBuilder<'a>,
            default: Option<&'a [Word]>,
        ) -> Result<Self> {
            builder
                .get_struct(INSTALL_REQUEST_SIZE, default)
                .map(Self::from)
        }
    }
}

pub mod install_response {
    use super::{install_error, *};

    #[derive(Clone, Copy)]
    pub struct Reader<'a> {
        reader: StructReader<'a>,
    }

    impl<'a> Reader<'a> {
        pub fn get_ok(&self) -> bool {
            self.reader.get_bool_field(0)
        }

        pub fn get_err(&self) -> install_error::Type {
            let raw = self.reader.get_data_field::<u16>(1);
            install_error::Type::from_u16(raw).unwrap_or(install_error::Type::Einval)
        }
    }

    impl<'a> From<StructReader<'a>> for Reader<'a> {
        fn from(reader: StructReader<'a>) -> Self {
            Self { reader }
        }
    }

    impl<'a> FromPointerReader<'a> for Reader<'a> {
        fn get_from_pointer(
            reader: &PointerReader<'a>,
            default: Option<&'a [Word]>,
        ) -> Result<Self> {
            reader.get_struct(default).map(Self::from)
        }
    }

    pub struct Builder<'a> {
        builder: StructBuilder<'a>,
    }

    impl<'a> Builder<'a> {
        pub fn set_ok(&mut self, value: bool) {
            self.builder.set_bool_field(0, value);
        }

        pub fn set_err(&mut self, value: install_error::Type) {
            self.builder.set_data_field::<u16>(1, value as u16);
        }
    }

    impl<'a> From<StructBuilder<'a>> for Builder<'a> {
        fn from(builder: StructBuilder<'a>) -> Self {
            Self { builder }
        }
    }

    impl<'a> FromPointerBuilder<'a> for Builder<'a> {
        fn init_pointer(builder: PointerBuilder<'a>, _size: u32) -> Self {
            builder.init_struct(INSTALL_RESPONSE_SIZE).into()
        }

        fn get_from_pointer(
            builder: PointerBuilder<'a>,
            default: Option<&'a [Word]>,
        ) -> Result<Self> {
            builder
                .get_struct(INSTALL_RESPONSE_SIZE, default)
                .map(Self::from)
        }
    }
}

pub mod query_request {
    use super::*;

    #[derive(Clone, Copy)]
    pub struct Reader<'a> {
        reader: StructReader<'a>,
    }

    impl<'a> Reader<'a> {
        pub fn get_name(&self) -> Result<&'a str> {
            self.reader
                .get_pointer_field(0)
                .get_text(None)?
                .to_str()
                .map_err(|err| capnp::Error::failed(err.to_string()))
        }
    }

    impl<'a> From<StructReader<'a>> for Reader<'a> {
        fn from(reader: StructReader<'a>) -> Self {
            Self { reader }
        }
    }

    impl<'a> FromPointerReader<'a> for Reader<'a> {
        fn get_from_pointer(
            reader: &PointerReader<'a>,
            default: Option<&'a [Word]>,
        ) -> Result<Self> {
            reader.get_struct(default).map(Self::from)
        }
    }

    pub struct Builder<'a> {
        builder: StructBuilder<'a>,
    }

    impl<'a> Builder<'a> {
        pub fn set_name(&mut self, value: &str) {
            self.builder
                .reborrow()
                .get_pointer_field(0)
                .set_text(value.into());
        }
    }

    impl<'a> From<StructBuilder<'a>> for Builder<'a> {
        fn from(builder: StructBuilder<'a>) -> Self {
            Self { builder }
        }
    }

    impl<'a> FromPointerBuilder<'a> for Builder<'a> {
        fn init_pointer(builder: PointerBuilder<'a>, _size: u32) -> Self {
            builder.init_struct(QUERY_REQUEST_SIZE).into()
        }

        fn get_from_pointer(
            builder: PointerBuilder<'a>,
            default: Option<&'a [Word]>,
        ) -> Result<Self> {
            builder
                .get_struct(QUERY_REQUEST_SIZE, default)
                .map(Self::from)
        }
    }
}

pub mod query_response {
    use super::*;

    #[derive(Clone, Copy)]
    pub struct Reader<'a> {
        reader: StructReader<'a>,
    }

    impl<'a> Reader<'a> {
        pub fn get_installed(&self) -> bool {
            self.reader.get_bool_field(0)
        }

        pub fn get_version(&self) -> Result<&'a str> {
            self.reader
                .get_pointer_field(0)
                .get_text(None)?
                .to_str()
                .map_err(|err| capnp::Error::failed(err.to_string()))
        }
    }

    impl<'a> From<StructReader<'a>> for Reader<'a> {
        fn from(reader: StructReader<'a>) -> Self {
            Self { reader }
        }
    }

    impl<'a> FromPointerReader<'a> for Reader<'a> {
        fn get_from_pointer(
            reader: &PointerReader<'a>,
            default: Option<&'a [Word]>,
        ) -> Result<Self> {
            reader.get_struct(default).map(Self::from)
        }
    }

    pub struct Builder<'a> {
        builder: StructBuilder<'a>,
    }

    impl<'a> Builder<'a> {
        pub fn set_installed(&mut self, value: bool) {
            self.builder.set_bool_field(0, value);
        }

        pub fn set_version(&mut self, value: &str) {
            self.builder
                .reborrow()
                .get_pointer_field(0)
                .set_text(value.into());
        }
    }

    impl<'a> From<StructBuilder<'a>> for Builder<'a> {
        fn from(builder: StructBuilder<'a>) -> Self {
            Self { builder }
        }
    }

    impl<'a> FromPointerBuilder<'a> for Builder<'a> {
        fn init_pointer(builder: PointerBuilder<'a>, _size: u32) -> Self {
            builder.init_struct(QUERY_RESPONSE_SIZE).into()
        }

        fn get_from_pointer(
            builder: PointerBuilder<'a>,
            default: Option<&'a [Word]>,
        ) -> Result<Self> {
            builder
                .get_struct(QUERY_RESPONSE_SIZE, default)
                .map(Self::from)
        }
    }
}
