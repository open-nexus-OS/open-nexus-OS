// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use capnp::private::layout::{PointerBuilder, PointerReader, StructBuilder, StructReader, StructSize};
use capnp::traits::{FromPointerBuilder, FromPointerReader};
use capnp::{Result, Word};

const CHECK_REQUEST_SIZE: StructSize = StructSize { data: 0, pointers: 2 };
const CHECK_RESPONSE_SIZE: StructSize = StructSize { data: 1, pointers: 1 };

pub mod check_request {
    use super::*;

    #[derive(Clone, Copy)]
    pub struct Reader<'a> {
        reader: StructReader<'a>,
    }

    impl<'a> Reader<'a> {
        pub fn get_subject(self) -> Result<::capnp::text::Reader<'a>> {
            ::capnp::traits::FromPointerReader::get_from_pointer(
                &self.reader.get_pointer_field(0),
                None,
            )
        }

        pub fn get_required_caps(self) -> Result<::capnp::text_list::Reader<'a>> {
            ::capnp::traits::FromPointerReader::get_from_pointer(
                &self.reader.get_pointer_field(1),
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
        pub fn set_subject(&mut self, value: &str) {
            self.builder
                .reborrow()
                .get_pointer_field(0)
                .set_text(value.into());
        }

        pub fn init_required_caps(&mut self, size: u32) -> ::capnp::text_list::Builder<'a> {
            self.builder
                .reborrow()
                .get_pointer_field(1)
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
            builder.init_struct(CHECK_REQUEST_SIZE).into()
        }

        fn get_from_pointer(
            builder: PointerBuilder<'a>,
            default: Option<&'a [Word]>,
        ) -> Result<Self> {
            builder
                .get_struct(CHECK_REQUEST_SIZE, default)
                .map(Self::from)
        }
    }
}

pub mod check_response {
    use super::*;

    #[derive(Clone, Copy)]
    pub struct Reader<'a> {
        reader: StructReader<'a>,
    }

    impl<'a> Reader<'a> {
        pub fn get_allowed(&self) -> bool {
            self.reader.get_bool_field(0)
        }

        pub fn get_missing(self) -> Result<::capnp::text_list::Reader<'a>> {
            ::capnp::traits::FromPointerReader::get_from_pointer(
                &self.reader.get_pointer_field(1),
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
        pub fn set_allowed(&mut self, value: bool) {
            self.builder.set_bool_field(0, value);
        }

        pub fn init_missing(&mut self, size: u32) -> ::capnp::text_list::Builder<'a> {
            self.builder
                .reborrow()
                .get_pointer_field(1)
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
            builder.init_struct(CHECK_RESPONSE_SIZE).into()
        }

        fn get_from_pointer(
            builder: PointerBuilder<'a>,
            default: Option<&'a [Word]>,
        ) -> Result<Self> {
            builder
                .get_struct(CHECK_RESPONSE_SIZE, default)
                .map(Self::from)
        }
    }
}
