// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use capnp::private::layout::{
    PointerBuilder, PointerReader, StructBuilder, StructReader, StructSize,
};
use capnp::traits::{FromPointerBuilder, FromPointerReader};
use capnp::{Result, Word};

const GET_ANCHORS_REQUEST_SIZE: StructSize = StructSize { data: 0, pointers: 0 };
const GET_ANCHORS_RESPONSE_SIZE: StructSize = StructSize { data: 0, pointers: 1 };
const VERIFY_REQUEST_SIZE: StructSize = StructSize { data: 0, pointers: 3 };
const VERIFY_RESPONSE_SIZE: StructSize = StructSize { data: 1, pointers: 0 };
const DEVICE_ID_REQUEST_SIZE: StructSize = StructSize { data: 0, pointers: 0 };
const DEVICE_ID_RESPONSE_SIZE: StructSize = StructSize { data: 0, pointers: 1 };

pub mod get_anchors_request {
    use super::*;

    #[derive(Clone, Copy)]
    pub struct Reader<'a> {
        reader: StructReader<'a>,
    }

    impl<'a> From<StructReader<'a>> for Reader<'a> {
        fn from(reader: StructReader<'a>) -> Self {
            Self { reader }
        }
    }

    impl<'a> FromPointerReader<'a> for Reader<'a> {
        fn get_from_pointer(reader: &PointerReader<'a>, default: Option<&'a [Word]>) -> Result<Self> {
            reader.get_struct(default).map(Self::from)
        }
    }

    pub struct Builder<'a> {
        builder: StructBuilder<'a>,
    }

    impl<'a> From<StructBuilder<'a>> for Builder<'a> {
        fn from(builder: StructBuilder<'a>) -> Self {
            Self { builder }
        }
    }

    impl<'a> FromPointerBuilder<'a> for Builder<'a> {
        fn init_pointer(builder: PointerBuilder<'a>, _size: u32) -> Self {
            builder.init_struct(GET_ANCHORS_REQUEST_SIZE).into()
        }

        fn get_from_pointer(
            builder: PointerBuilder<'a>,
            default: Option<&'a [Word]>,
        ) -> Result<Self> {
            builder
                .get_struct(GET_ANCHORS_REQUEST_SIZE, default)
                .map(Self::from)
        }
    }
}

pub mod get_anchors_response {
    use super::*;

    #[derive(Clone, Copy)]
    pub struct Reader<'a> {
        reader: StructReader<'a>,
    }

    impl<'a> Reader<'a> {
        pub fn get_anchors(
            self,
        ) -> Result<::capnp::text_list::Reader<'a>> {
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
        fn get_from_pointer(reader: &PointerReader<'a>, default: Option<&'a [Word]>) -> Result<Self> {
            reader.get_struct(default).map(Self::from)
        }
    }

    pub struct Builder<'a> {
        builder: StructBuilder<'a>,
    }

    impl<'a> Builder<'a> {
        pub fn init_anchors(&mut self, size: u32) -> ::capnp::text_list::Builder<'a> {
            self.builder
                .reborrow()
                .get_pointer_field(0)
                .init_text_list(size)
        }
    }

    impl<'a> From<StructBuilder<'a>> for Builder<'a> {
        fn from(builder: StructBuilder<'a>) -> Self {
            Self { builder }
        }
    }

    impl<'a> FromPointerBuilder<'a> for Builder<'a> {
        fn init_pointer(builder: PointerBuilder<'a>, _size: u32) -> Self {
            builder.init_struct(GET_ANCHORS_RESPONSE_SIZE).into()
        }

        fn get_from_pointer(
            builder: PointerBuilder<'a>,
            default: Option<&'a [Word]>,
        ) -> Result<Self> {
            builder
                .get_struct(GET_ANCHORS_RESPONSE_SIZE, default)
                .map(Self::from)
        }
    }
}

pub mod verify_request {
    use super::*;

    #[derive(Clone, Copy)]
    pub struct Reader<'a> {
        reader: StructReader<'a>,
    }

    impl<'a> Reader<'a> {
        pub fn get_anchor_id(self) -> Result<::capnp::text::Reader<'a>> {
            ::capnp::traits::FromPointerReader::get_from_pointer(
                &self.reader.get_pointer_field(0),
                None,
            )
        }

        pub fn get_payload(self) -> Result<::capnp::data::Reader<'a>> {
            ::capnp::traits::FromPointerReader::get_from_pointer(
                &self.reader.get_pointer_field(1),
                None,
            )
        }

        pub fn get_signature(self) -> Result<::capnp::data::Reader<'a>> {
            ::capnp::traits::FromPointerReader::get_from_pointer(
                &self.reader.get_pointer_field(2),
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
        fn get_from_pointer(reader: &PointerReader<'a>, default: Option<&'a [Word]>) -> Result<Self> {
            reader.get_struct(default).map(Self::from)
        }
    }

    pub struct Builder<'a> {
        builder: StructBuilder<'a>,
    }

    impl<'a> Builder<'a> {
        pub fn set_anchor_id(&mut self, value: &str) {
            self.builder
                .reborrow()
                .get_pointer_field(0)
                .set_text(value.into());
        }

        pub fn set_payload(&mut self, value: &[u8]) {
            self.builder
                .reborrow()
                .get_pointer_field(1)
                .set_data(value);
        }

        pub fn set_signature(&mut self, value: &[u8]) {
            self.builder
                .reborrow()
                .get_pointer_field(2)
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
            builder.init_struct(VERIFY_REQUEST_SIZE).into()
        }

        fn get_from_pointer(
            builder: PointerBuilder<'a>,
            default: Option<&'a [Word]>,
        ) -> Result<Self> {
            builder
                .get_struct(VERIFY_REQUEST_SIZE, default)
                .map(Self::from)
        }
    }
}

pub mod verify_response {
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
        fn get_from_pointer(reader: &PointerReader<'a>, default: Option<&'a [Word]>) -> Result<Self> {
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
            builder.init_struct(VERIFY_RESPONSE_SIZE).into()
        }

        fn get_from_pointer(
            builder: PointerBuilder<'a>,
            default: Option<&'a [Word]>,
        ) -> Result<Self> {
            builder
                .get_struct(VERIFY_RESPONSE_SIZE, default)
                .map(Self::from)
        }
    }
}

pub mod device_id_request {
    use super::*;

    #[derive(Clone, Copy)]
    pub struct Reader<'a> {
        reader: StructReader<'a>,
    }

    impl<'a> From<StructReader<'a>> for Reader<'a> {
        fn from(reader: StructReader<'a>) -> Self {
            Self { reader }
        }
    }

    impl<'a> FromPointerReader<'a> for Reader<'a> {
        fn get_from_pointer(reader: &PointerReader<'a>, default: Option<&'a [Word]>) -> Result<Self> {
            reader.get_struct(default).map(Self::from)
        }
    }

    pub struct Builder<'a> {
        builder: StructBuilder<'a>,
    }

    impl<'a> From<StructBuilder<'a>> for Builder<'a> {
        fn from(builder: StructBuilder<'a>) -> Self {
            Self { builder }
        }
    }

    impl<'a> FromPointerBuilder<'a> for Builder<'a> {
        fn init_pointer(builder: PointerBuilder<'a>, _size: u32) -> Self {
            builder.init_struct(DEVICE_ID_REQUEST_SIZE).into()
        }

        fn get_from_pointer(
            builder: PointerBuilder<'a>,
            default: Option<&'a [Word]>,
        ) -> Result<Self> {
            builder
                .get_struct(DEVICE_ID_REQUEST_SIZE, default)
                .map(Self::from)
        }
    }
}

pub mod device_id_response {
    use super::*;

    #[derive(Clone, Copy)]
    pub struct Reader<'a> {
        reader: StructReader<'a>,
    }

    impl<'a> Reader<'a> {
        pub fn get_id(self) -> Result<::capnp::text::Reader<'a>> {
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
        fn get_from_pointer(reader: &PointerReader<'a>, default: Option<&'a [Word]>) -> Result<Self> {
            reader.get_struct(default).map(Self::from)
        }
    }

    pub struct Builder<'a> {
        builder: StructBuilder<'a>,
    }

    impl<'a> Builder<'a> {
        pub fn set_id(&mut self, value: &str) {
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
            builder.init_struct(DEVICE_ID_RESPONSE_SIZE).into()
        }

        fn get_from_pointer(
            builder: PointerBuilder<'a>,
            default: Option<&'a [Word]>,
        ) -> Result<Self> {
            builder
                .get_struct(DEVICE_ID_RESPONSE_SIZE, default)
                .map(Self::from)
        }
    }
}
