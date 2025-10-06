// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use capnp::private::layout::{
    PointerBuilder, PointerReader, StructBuilder, StructReader, StructSize,
};
use capnp::traits::{FromPointerBuilder, FromPointerReader};
use capnp::{Result, Word};

const REGISTER_REQUEST_SIZE: StructSize = StructSize {
    data: 1,
    pointers: 1,
};
const REGISTER_RESPONSE_SIZE: StructSize = StructSize {
    data: 1,
    pointers: 0,
};
const RESOLVE_REQUEST_SIZE: StructSize = StructSize {
    data: 0,
    pointers: 1,
};
const RESOLVE_RESPONSE_SIZE: StructSize = StructSize {
    data: 1,
    pointers: 0,
};
const HEARTBEAT_SIZE: StructSize = StructSize {
    data: 1,
    pointers: 0,
};

pub mod register_request {
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

        pub fn get_endpoint(&self) -> u32 {
            self.reader.get_data_field::<u32>(0)
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

        pub fn set_endpoint(&mut self, value: u32) {
            self.builder.set_data_field::<u32>(0, value);
        }
    }

    impl<'a> From<StructBuilder<'a>> for Builder<'a> {
        fn from(builder: StructBuilder<'a>) -> Self {
            Self { builder }
        }
    }

    impl<'a> FromPointerBuilder<'a> for Builder<'a> {
        fn init_pointer(builder: PointerBuilder<'a>, _size: u32) -> Self {
            builder.init_struct(REGISTER_REQUEST_SIZE).into()
        }

        fn get_from_pointer(
            builder: PointerBuilder<'a>,
            default: Option<&'a [Word]>,
        ) -> Result<Self> {
            builder
                .get_struct(REGISTER_REQUEST_SIZE, default)
                .map(Self::from)
        }
    }
}

pub mod register_response {
    use super::*;

    #[derive(Clone, Copy)]
    pub struct Reader<'a> {
        reader: StructReader<'a>,
    }

    impl<'a> Reader<'a> {
        pub fn get_ok(&self) -> bool {
            self.reader.get_bool_field(0)
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
    }

    impl<'a> From<StructBuilder<'a>> for Builder<'a> {
        fn from(builder: StructBuilder<'a>) -> Self {
            Self { builder }
        }
    }

    impl<'a> FromPointerBuilder<'a> for Builder<'a> {
        fn init_pointer(builder: PointerBuilder<'a>, _size: u32) -> Self {
            builder.init_struct(REGISTER_RESPONSE_SIZE).into()
        }

        fn get_from_pointer(
            builder: PointerBuilder<'a>,
            default: Option<&'a [Word]>,
        ) -> Result<Self> {
            builder
                .get_struct(REGISTER_RESPONSE_SIZE, default)
                .map(Self::from)
        }
    }
}

pub mod resolve_request {
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
            builder.init_struct(RESOLVE_REQUEST_SIZE).into()
        }

        fn get_from_pointer(
            builder: PointerBuilder<'a>,
            default: Option<&'a [Word]>,
        ) -> Result<Self> {
            builder
                .get_struct(RESOLVE_REQUEST_SIZE, default)
                .map(Self::from)
        }
    }
}

pub mod resolve_response {
    use super::*;

    #[derive(Clone, Copy)]
    pub struct Reader<'a> {
        reader: StructReader<'a>,
    }

    impl<'a> Reader<'a> {
        pub fn get_endpoint(&self) -> u32 {
            self.reader.get_data_field::<u32>(0)
        }

        pub fn get_found(&self) -> bool {
            self.reader.get_bool_field(32)
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
        pub fn set_endpoint(&mut self, value: u32) {
            self.builder.set_data_field::<u32>(0, value);
        }

        pub fn set_found(&mut self, value: bool) {
            self.builder.set_bool_field(32, value);
        }
    }

    impl<'a> From<StructBuilder<'a>> for Builder<'a> {
        fn from(builder: StructBuilder<'a>) -> Self {
            Self { builder }
        }
    }

    impl<'a> FromPointerBuilder<'a> for Builder<'a> {
        fn init_pointer(builder: PointerBuilder<'a>, _size: u32) -> Self {
            builder.init_struct(RESOLVE_RESPONSE_SIZE).into()
        }

        fn get_from_pointer(
            builder: PointerBuilder<'a>,
            default: Option<&'a [Word]>,
        ) -> Result<Self> {
            builder
                .get_struct(RESOLVE_RESPONSE_SIZE, default)
                .map(Self::from)
        }
    }
}

pub mod heartbeat {
    use super::*;

    #[derive(Clone, Copy)]
    pub struct Reader<'a> {
        reader: StructReader<'a>,
    }

    impl<'a> Reader<'a> {
        pub fn get_endpoint(&self) -> u32 {
            self.reader.get_data_field::<u32>(0)
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
        pub fn set_endpoint(&mut self, value: u32) {
            self.builder.set_data_field::<u32>(0, value);
        }
    }

    impl<'a> From<StructBuilder<'a>> for Builder<'a> {
        fn from(builder: StructBuilder<'a>) -> Self {
            Self { builder }
        }
    }

    impl<'a> FromPointerBuilder<'a> for Builder<'a> {
        fn init_pointer(builder: PointerBuilder<'a>, _size: u32) -> Self {
            builder.init_struct(HEARTBEAT_SIZE).into()
        }

        fn get_from_pointer(
            builder: PointerBuilder<'a>,
            default: Option<&'a [Word]>,
        ) -> Result<Self> {
            builder.get_struct(HEARTBEAT_SIZE, default).map(Self::from)
        }
    }
}
