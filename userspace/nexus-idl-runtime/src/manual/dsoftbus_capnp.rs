// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use capnp::private::layout::{
    PointerBuilder, PointerReader, StructBuilder, StructReader, StructSize,
};
use capnp::traits::{FromPointerBuilder, FromPointerReader};
use capnp::{Result, Word};

const ANNOUNCE_SIZE: StructSize = StructSize {
    data: 1,
    pointers: 2,
};
const CONNECT_REQUEST_SIZE: StructSize = StructSize {
    data: 0,
    pointers: 1,
};
const CONNECT_RESPONSE_SIZE: StructSize = StructSize {
    data: 1,
    pointers: 0,
};
const FRAME_SIZE: StructSize = StructSize {
    data: 1,
    pointers: 1,
};

pub mod announce {
    use super::*;

    #[derive(Clone, Copy)]
    pub struct Reader<'a> {
        reader: StructReader<'a>,
    }

    impl<'a> Reader<'a> {
        pub fn get_device_id(self) -> Result<::capnp::text::Reader<'a>> {
            ::capnp::traits::FromPointerReader::get_from_pointer(
                &self.reader.get_pointer_field(0),
                None,
            )
        }

        pub fn get_services(self) -> Result<::capnp::text_list::Reader<'a>> {
            ::capnp::traits::FromPointerReader::get_from_pointer(
                &self.reader.get_pointer_field(1),
                None,
            )
        }

        pub fn get_port(&self) -> u16 {
            self.reader.get_data_field::<u16>(0)
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
        pub fn set_device_id(&mut self, value: &str) {
            self.builder
                .reborrow()
                .get_pointer_field(0)
                .set_text(value.into());
        }

        pub fn init_services(&mut self, size: u32) -> ::capnp::text_list::Builder<'a> {
            self.builder
                .reborrow()
                .get_pointer_field(1)
                .init_text_list(size)
        }

        pub fn set_port(&mut self, value: u16) {
            self.builder.set_data_field::<u16>(0, value);
        }
    }

    impl<'a> From<StructBuilder<'a>> for Builder<'a> {
        fn from(builder: StructBuilder<'a>) -> Self {
            Self { builder }
        }
    }

    impl<'a> FromPointerBuilder<'a> for Builder<'a> {
        fn init_pointer(builder: PointerBuilder<'a>, _size: u32) -> Self {
            builder.init_struct(ANNOUNCE_SIZE).into()
        }

        fn get_from_pointer(
            builder: PointerBuilder<'a>,
            default: Option<&'a [Word]>,
        ) -> Result<Self> {
            builder.get_struct(ANNOUNCE_SIZE, default).map(Self::from)
        }
    }
}

pub mod connect_request {
    use super::*;

    #[derive(Clone, Copy)]
    pub struct Reader<'a> {
        reader: StructReader<'a>,
    }

    impl<'a> Reader<'a> {
        pub fn get_device_id(self) -> Result<::capnp::text::Reader<'a>> {
            ::capnp::traits::FromPointerReader::get_from_pointer(
                &self.reader.get_pointer_field(0),
                None,
            )
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
        pub fn set_device_id(&mut self, value: &str) {
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
            builder.init_struct(CONNECT_REQUEST_SIZE).into()
        }

        fn get_from_pointer(
            builder: PointerBuilder<'a>,
            default: Option<&'a [Word]>,
        ) -> Result<Self> {
            builder
                .get_struct(CONNECT_REQUEST_SIZE, default)
                .map(Self::from)
        }
    }
}

pub mod connect_response {
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
            builder.init_struct(CONNECT_RESPONSE_SIZE).into()
        }

        fn get_from_pointer(
            builder: PointerBuilder<'a>,
            default: Option<&'a [Word]>,
        ) -> Result<Self> {
            builder
                .get_struct(CONNECT_RESPONSE_SIZE, default)
                .map(Self::from)
        }
    }
}

pub mod frame {
    use super::*;

    #[derive(Clone, Copy)]
    pub struct Reader<'a> {
        reader: StructReader<'a>,
    }

    impl<'a> Reader<'a> {
        pub fn get_chan(&self) -> u32 {
            self.reader.get_data_field::<u32>(0)
        }

        pub fn get_bytes(self) -> Result<::capnp::data::Reader<'a>> {
            ::capnp::traits::FromPointerReader::get_from_pointer(
                &self.reader.get_pointer_field(0),
                None,
            )
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
        pub fn set_chan(&mut self, value: u32) {
            self.builder.set_data_field::<u32>(0, value);
        }

        pub fn set_bytes(&mut self, value: &[u8]) {
            self.builder
                .reborrow()
                .get_pointer_field(0)
                .set_data(value);
        }
    }

    impl<'a> From<StructBuilder<'a>> for Builder<'a> {
        fn from(builder: StructBuilder<'a>) -> Self {
            Self { builder }
        }
    }

    impl<'a> FromPointerBuilder<'a> for Builder<'a> {
        fn init_pointer(builder: PointerBuilder<'a>, _size: u32) -> Self {
            builder.init_struct(FRAME_SIZE).into()
        }

        fn get_from_pointer(
            builder: PointerBuilder<'a>,
            default: Option<&'a [Word]>,
        ) -> Result<Self> {
            builder.get_struct(FRAME_SIZE, default).map(Self::from)
        }
    }
}
