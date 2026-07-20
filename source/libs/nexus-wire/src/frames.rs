// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `frames!` — the declarative frame DSL over [`crate::codec`] (ADR-0051)
//! OWNERS: @runtime
//! PUBLIC API: frames! (exported at the crate root)
//! DEPENDS_ON: crate::codec
//! INVARIANTS: generated encoders/decoders expand to the same straight-line
//!             byte operations the hand-written codecs used — golden-byte
//!             tests are the equivalence gate; decoders are exact-length and
//!             fail-closed; `lit` bytes are checked on decode, `pad` bytes are
//!             skipped unchecked (matches the historical acceptance sets)

/// Declares a protocol's wire frames; generates `encode_*`/`decode_*` fns.
///
/// Grammar (trailing commas after every field are required):
///
/// ```text
/// frames! {
///     protocol(magic0 = MAGIC0, magic1 = MAGIC1, version = VERSION);
///
///     /// Item docs are attached to both generated fns.
///     request encode_x / decode_x (op = OP_X) { field: kind, ... }
///     reply   encode_y / decode_y (op = OP_X) { ... }          // wire op = OP_X | 0x80
///     request fixed encode_z / decode_z (op = OP_Z) { ... }    // encode returns [u8; N]
///     request encode only_enc (op = OP_A) { ... }              // one-sided forms:
///     request decode only_dec (op = OP_A) { ... }              //   encode / decode / fixed encode
///     reply   encode_r / decode_r (op = caller) { ... }        // op is a runtime `op: u8` param
///     request encode_v (op = OP_B, version = VERSION_V2) { ... } // per-item version override
/// }
/// ```
///
/// Field kinds (all little-endian):
/// - `u8`, `u16le`, `u32le`, `u64le` — scalars
/// - `nz_u8` — `u8` that must be non-zero (encode and decode reject 0)
/// - `lit(EXPR)` — literal byte: written on encode, **checked** on decode
/// - `pad(N)` — `N` reserved bytes: zeroed on encode, **skipped** on decode
/// - `str8(min = A, max = B)` / `bytes8(min = A, max = B)` — `u8`-length-prefixed
///   UTF-8 str / byte field with inclusive bounds
/// - `bytes16(...)` / `bytes32(...)` — `u16le` / `u32le`-length-prefixed bytes
///
/// Generated signatures: variable frames encode into a caller buffer
/// (`fn(fields.., out: &mut [u8]) -> Option<usize>`); `fixed` frames return
/// the exact array (`fn(fields..) -> [u8; N]`, all fields fixed-width).
/// Decoders take `frame: &[u8]`, apply the magic/version/op guard, read the
/// fields, require exact length, and return the value fields (a bare value
/// for one field, a tuple for several).
#[macro_export]
macro_rules! frames {
    (
        protocol(magic0 = $m0:expr, magic1 = $m1:expr, version = $pv:expr);
        $($items:tt)*
    ) => {
        $crate::frames!(@items ($m0, $m1, $pv) $($items)*);
    };

    // ---- item dispatch (specific forms before the generic two-sided one) ----
    (@items $cfg:tt) => {};
    (@items $cfg:tt $(#[$m:meta])* request encode $enc:ident $opts:tt { $($f:tt)* } $($rest:tt)*) => {
        $crate::frames!(@enc $cfg [req] [var] [$(#[$m])*] $enc $opts { $($f)* });
        $crate::frames!(@items $cfg $($rest)*);
    };
    (@items $cfg:tt $(#[$m:meta])* request decode $dec:ident $opts:tt { $($f:tt)* } $($rest:tt)*) => {
        $crate::frames!(@dec $cfg [req] [$(#[$m])*] $dec $opts { $($f)* });
        $crate::frames!(@items $cfg $($rest)*);
    };
    (@items $cfg:tt $(#[$m:meta])* request fixed encode $enc:ident $opts:tt { $($f:tt)* } $($rest:tt)*) => {
        $crate::frames!(@enc $cfg [req] [fix] [$(#[$m])*] $enc $opts { $($f)* });
        $crate::frames!(@items $cfg $($rest)*);
    };
    (@items $cfg:tt $(#[$m:meta])* request fixed $enc:ident / $dec:ident $opts:tt { $($f:tt)* } $($rest:tt)*) => {
        $crate::frames!(@enc $cfg [req] [fix] [$(#[$m])*] $enc $opts { $($f)* });
        $crate::frames!(@dec $cfg [req] [$(#[$m])*] $dec $opts { $($f)* });
        $crate::frames!(@items $cfg $($rest)*);
    };
    (@items $cfg:tt $(#[$m:meta])* request $enc:ident / $dec:ident $opts:tt { $($f:tt)* } $($rest:tt)*) => {
        $crate::frames!(@enc $cfg [req] [var] [$(#[$m])*] $enc $opts { $($f)* });
        $crate::frames!(@dec $cfg [req] [$(#[$m])*] $dec $opts { $($f)* });
        $crate::frames!(@items $cfg $($rest)*);
    };
    (@items $cfg:tt $(#[$m:meta])* reply encode $enc:ident $opts:tt { $($f:tt)* } $($rest:tt)*) => {
        $crate::frames!(@enc $cfg [rsp] [var] [$(#[$m])*] $enc $opts { $($f)* });
        $crate::frames!(@items $cfg $($rest)*);
    };
    (@items $cfg:tt $(#[$m:meta])* reply decode $dec:ident $opts:tt { $($f:tt)* } $($rest:tt)*) => {
        $crate::frames!(@dec $cfg [rsp] [$(#[$m])*] $dec $opts { $($f)* });
        $crate::frames!(@items $cfg $($rest)*);
    };
    (@items $cfg:tt $(#[$m:meta])* reply fixed encode $enc:ident $opts:tt { $($f:tt)* } $($rest:tt)*) => {
        $crate::frames!(@enc $cfg [rsp] [fix] [$(#[$m])*] $enc $opts { $($f)* });
        $crate::frames!(@items $cfg $($rest)*);
    };
    (@items $cfg:tt $(#[$m:meta])* reply fixed $enc:ident / $dec:ident $opts:tt { $($f:tt)* } $($rest:tt)*) => {
        $crate::frames!(@enc $cfg [rsp] [fix] [$(#[$m])*] $enc $opts { $($f)* });
        $crate::frames!(@dec $cfg [rsp] [$(#[$m])*] $dec $opts { $($f)* });
        $crate::frames!(@items $cfg $($rest)*);
    };
    (@items $cfg:tt $(#[$m:meta])* reply $enc:ident / $dec:ident $opts:tt { $($f:tt)* } $($rest:tt)*) => {
        $crate::frames!(@enc $cfg [rsp] [var] [$(#[$m])*] $enc $opts { $($f)* });
        $crate::frames!(@dec $cfg [rsp] [$(#[$m])*] $dec $opts { $($f)* });
        $crate::frames!(@items $cfg $($rest)*);
    };

    // ---- option resolution (op = caller | expr, optional version override) ----
    (@enc $cfg:tt $dir:tt $form:tt $metas:tt $enc:ident (op = caller $(, version = $v:expr)?) { $($f:tt)* }) => {
        $crate::frames!(@encm $cfg $form $metas $enc [op: u8,]
            ($crate::frames!(@wireop $dir op)) ($crate::frames!(@ver $cfg $($v)?))
            w [] [] [] $($f)*);
    };
    (@enc $cfg:tt $dir:tt $form:tt $metas:tt $enc:ident (op = $op:expr $(, version = $v:expr)?) { $($f:tt)* }) => {
        $crate::frames!(@encm $cfg $form $metas $enc []
            ($crate::frames!(@wireop $dir $op)) ($crate::frames!(@ver $cfg $($v)?))
            w [] [] [] $($f)*);
    };
    (@dec $cfg:tt $dir:tt $metas:tt $dec:ident (op = caller $(, version = $v:expr)?) { $($f:tt)* }) => {
        $crate::frames!(@decm $cfg $metas $dec [op: u8,]
            ($crate::frames!(@wireop $dir op)) ($crate::frames!(@ver $cfg $($v)?))
            r [] [] $($f)*);
    };
    (@dec $cfg:tt $dir:tt $metas:tt $dec:ident (op = $op:expr $(, version = $v:expr)?) { $($f:tt)* }) => {
        $crate::frames!(@decm $cfg $metas $dec []
            ($crate::frames!(@wireop $dir $op)) ($crate::frames!(@ver $cfg $($v)?))
            r [] [] $($f)*);
    };

    (@wireop [req] $op:expr) => { $op };
    (@wireop [rsp] $op:expr) => { ($op | $crate::codec::REPLY_BIT) };
    (@ver ($m0:expr, $m1:expr, $pv:expr)) => { $pv };
    (@ver ($m0:expr, $m1:expr, $pv:expr) $v:expr) => { $v };

    // ---- encoder field muncher: [args] [stmts] [size] ----
    (@encm $cfg:tt $form:tt $metas:tt $enc:ident $x:tt $wireop:tt $ver:tt $w:ident
        [$($a:tt)*] [$($s:tt)*] [$($z:tt)*] $n:ident: u8, $($rest:tt)*) => {
        $crate::frames!(@encm $cfg $form $metas $enc $x $wireop $ver $w
            [$($a)* $n: u8,] [$($s)* $w.put_u8($n)?;] [$($z)* + 1] $($rest)*);
    };
    (@encm $cfg:tt $form:tt $metas:tt $enc:ident $x:tt $wireop:tt $ver:tt $w:ident
        [$($a:tt)*] [$($s:tt)*] [$($z:tt)*] $n:ident: nz_u8, $($rest:tt)*) => {
        $crate::frames!(@encm $cfg $form $metas $enc $x $wireop $ver $w
            [$($a)* $n: u8,] [$($s)* $w.put_nz_u8($n)?;] [$($z)* + 1] $($rest)*);
    };
    (@encm $cfg:tt $form:tt $metas:tt $enc:ident $x:tt $wireop:tt $ver:tt $w:ident
        [$($a:tt)*] [$($s:tt)*] [$($z:tt)*] $n:ident: u16le, $($rest:tt)*) => {
        $crate::frames!(@encm $cfg $form $metas $enc $x $wireop $ver $w
            [$($a)* $n: u16,] [$($s)* $w.put_u16le($n)?;] [$($z)* + 2] $($rest)*);
    };
    (@encm $cfg:tt $form:tt $metas:tt $enc:ident $x:tt $wireop:tt $ver:tt $w:ident
        [$($a:tt)*] [$($s:tt)*] [$($z:tt)*] $n:ident: u32le, $($rest:tt)*) => {
        $crate::frames!(@encm $cfg $form $metas $enc $x $wireop $ver $w
            [$($a)* $n: u32,] [$($s)* $w.put_u32le($n)?;] [$($z)* + 4] $($rest)*);
    };
    (@encm $cfg:tt $form:tt $metas:tt $enc:ident $x:tt $wireop:tt $ver:tt $w:ident
        [$($a:tt)*] [$($s:tt)*] [$($z:tt)*] $n:ident: u64le, $($rest:tt)*) => {
        $crate::frames!(@encm $cfg $form $metas $enc $x $wireop $ver $w
            [$($a)* $n: u64,] [$($s)* $w.put_u64le($n)?;] [$($z)* + 8] $($rest)*);
    };
    (@encm $cfg:tt $form:tt $metas:tt $enc:ident $x:tt $wireop:tt $ver:tt $w:ident
        [$($a:tt)*] [$($s:tt)*] [$($z:tt)*] $n:ident: lit($e:expr), $($rest:tt)*) => {
        $crate::frames!(@encm $cfg $form $metas $enc $x $wireop $ver $w
            [$($a)*] [$($s)* $w.put_u8($e)?;] [$($z)* + 1] $($rest)*);
    };
    (@encm $cfg:tt $form:tt $metas:tt $enc:ident $x:tt $wireop:tt $ver:tt $w:ident
        [$($a:tt)*] [$($s:tt)*] [$($z:tt)*] $n:ident: pad($e:expr), $($rest:tt)*) => {
        $crate::frames!(@encm $cfg $form $metas $enc $x $wireop $ver $w
            [$($a)*] [$($s)* $w.put_pad($e)?;] [$($z)* + $e] $($rest)*);
    };
    (@encm $cfg:tt $form:tt $metas:tt $enc:ident $x:tt $wireop:tt $ver:tt $w:ident
        [$($a:tt)*] [$($s:tt)*] [$($z:tt)*] $n:ident: str8(min = $min:expr, max = $max:expr), $($rest:tt)*) => {
        $crate::frames!(@encm $cfg $form $metas $enc $x $wireop $ver $w
            [$($a)* $n: &str,] [$($s)* $w.put_len8_str($n, $min, $max)?;]
            [$($z)* + { compile_error!("variable-length fields cannot appear in fixed frames") }]
            $($rest)*);
    };
    (@encm $cfg:tt $form:tt $metas:tt $enc:ident $x:tt $wireop:tt $ver:tt $w:ident
        [$($a:tt)*] [$($s:tt)*] [$($z:tt)*] $n:ident: bytes8(min = $min:expr, max = $max:expr), $($rest:tt)*) => {
        $crate::frames!(@encm $cfg $form $metas $enc $x $wireop $ver $w
            [$($a)* $n: &[u8],] [$($s)* $w.put_len8_bytes($n, $min, $max)?;]
            [$($z)* + { compile_error!("variable-length fields cannot appear in fixed frames") }]
            $($rest)*);
    };
    (@encm $cfg:tt $form:tt $metas:tt $enc:ident $x:tt $wireop:tt $ver:tt $w:ident
        [$($a:tt)*] [$($s:tt)*] [$($z:tt)*] $n:ident: bytes16(min = $min:expr, max = $max:expr), $($rest:tt)*) => {
        $crate::frames!(@encm $cfg $form $metas $enc $x $wireop $ver $w
            [$($a)* $n: &[u8],] [$($s)* $w.put_len16_bytes($n, $min, $max)?;]
            [$($z)* + { compile_error!("variable-length fields cannot appear in fixed frames") }]
            $($rest)*);
    };
    (@encm $cfg:tt $form:tt $metas:tt $enc:ident $x:tt $wireop:tt $ver:tt $w:ident
        [$($a:tt)*] [$($s:tt)*] [$($z:tt)*] $n:ident: bytes32(min = $min:expr, max = $max:expr), $($rest:tt)*) => {
        $crate::frames!(@encm $cfg $form $metas $enc $x $wireop $ver $w
            [$($a)* $n: &[u8],] [$($s)* $w.put_len32_bytes($n, $min, $max)?;]
            [$($z)* + { compile_error!("variable-length fields cannot appear in fixed frames") }]
            $($rest)*);
    };

    // encoder terminals
    (@encm ($m0:expr, $m1:expr, $pv:expr) [var] [$(#[$m:meta])*] $enc:ident [$($x:tt)*] $wireop:tt $ver:tt $w:ident
        [$($a:tt)*] [$($s:tt)*] [$($z:tt)*]) => {
        $(#[$m])*
        pub fn $enc($($x)* $($a)* out: &mut [u8]) -> Option<usize> {
            let mut $w = $crate::codec::Writer::new(out);
            $crate::codec::put_hdr(&mut $w, $m0, $m1, $ver, $wireop)?;
            $($s)*
            Some($w.pos())
        }
    };
    (@encm ($m0:expr, $m1:expr, $pv:expr) [fix] [$(#[$m:meta])*] $enc:ident [$($x:tt)*] $wireop:tt $ver:tt $w:ident
        [$($a:tt)*] [$($s:tt)*] [$($z:tt)*]) => {
        $(#[$m])*
        pub fn $enc($($x)* $($a)*) -> [u8; { 4 $($z)* }] {
            let mut out = [0u8; { 4 $($z)* }];
            {
                let mut $w = $crate::codec::Writer::new(&mut out);
                let ok = $crate::codec::build(|| {
                    $crate::codec::put_hdr(&mut $w, $m0, $m1, $ver, $wireop)?;
                    $($s)*
                    Some(())
                });
                debug_assert!(ok, "frames!: fixed-size frame must always encode");
            }
            out
        }
    };

    // ---- decoder field muncher: [stmts] [(name, type) rets] ----
    (@decm $cfg:tt $metas:tt $dec:ident $x:tt $wireop:tt $ver:tt $r:ident
        [$($s:tt)*] [$($t:tt)*] $n:ident: u8, $($rest:tt)*) => {
        $crate::frames!(@decm $cfg $metas $dec $x $wireop $ver $r
            [$($s)* let $n = $r.take_u8()?;] [$($t)* ($n, u8)] $($rest)*);
    };
    (@decm $cfg:tt $metas:tt $dec:ident $x:tt $wireop:tt $ver:tt $r:ident
        [$($s:tt)*] [$($t:tt)*] $n:ident: nz_u8, $($rest:tt)*) => {
        $crate::frames!(@decm $cfg $metas $dec $x $wireop $ver $r
            [$($s)* let $n = $r.take_nz_u8()?;] [$($t)* ($n, u8)] $($rest)*);
    };
    (@decm $cfg:tt $metas:tt $dec:ident $x:tt $wireop:tt $ver:tt $r:ident
        [$($s:tt)*] [$($t:tt)*] $n:ident: u16le, $($rest:tt)*) => {
        $crate::frames!(@decm $cfg $metas $dec $x $wireop $ver $r
            [$($s)* let $n = $r.take_u16le()?;] [$($t)* ($n, u16)] $($rest)*);
    };
    (@decm $cfg:tt $metas:tt $dec:ident $x:tt $wireop:tt $ver:tt $r:ident
        [$($s:tt)*] [$($t:tt)*] $n:ident: u32le, $($rest:tt)*) => {
        $crate::frames!(@decm $cfg $metas $dec $x $wireop $ver $r
            [$($s)* let $n = $r.take_u32le()?;] [$($t)* ($n, u32)] $($rest)*);
    };
    (@decm $cfg:tt $metas:tt $dec:ident $x:tt $wireop:tt $ver:tt $r:ident
        [$($s:tt)*] [$($t:tt)*] $n:ident: u64le, $($rest:tt)*) => {
        $crate::frames!(@decm $cfg $metas $dec $x $wireop $ver $r
            [$($s)* let $n = $r.take_u64le()?;] [$($t)* ($n, u64)] $($rest)*);
    };
    (@decm $cfg:tt $metas:tt $dec:ident $x:tt $wireop:tt $ver:tt $r:ident
        [$($s:tt)*] [$($t:tt)*] $n:ident: lit($e:expr), $($rest:tt)*) => {
        $crate::frames!(@decm $cfg $metas $dec $x $wireop $ver $r
            [$($s)* $r.expect_u8($e)?;] [$($t)*] $($rest)*);
    };
    (@decm $cfg:tt $metas:tt $dec:ident $x:tt $wireop:tt $ver:tt $r:ident
        [$($s:tt)*] [$($t:tt)*] $n:ident: pad($e:expr), $($rest:tt)*) => {
        $crate::frames!(@decm $cfg $metas $dec $x $wireop $ver $r
            [$($s)* $r.skip($e)?;] [$($t)*] $($rest)*);
    };
    (@decm $cfg:tt $metas:tt $dec:ident $x:tt $wireop:tt $ver:tt $r:ident
        [$($s:tt)*] [$($t:tt)*] $n:ident: str8(min = $min:expr, max = $max:expr), $($rest:tt)*) => {
        $crate::frames!(@decm $cfg $metas $dec $x $wireop $ver $r
            [$($s)* let $n = $r.take_len8_str($min, $max)?;] [$($t)* ($n, &str)] $($rest)*);
    };
    (@decm $cfg:tt $metas:tt $dec:ident $x:tt $wireop:tt $ver:tt $r:ident
        [$($s:tt)*] [$($t:tt)*] $n:ident: bytes8(min = $min:expr, max = $max:expr), $($rest:tt)*) => {
        $crate::frames!(@decm $cfg $metas $dec $x $wireop $ver $r
            [$($s)* let $n = $r.take_len8_bytes($min, $max)?;] [$($t)* ($n, &[u8])] $($rest)*);
    };
    (@decm $cfg:tt $metas:tt $dec:ident $x:tt $wireop:tt $ver:tt $r:ident
        [$($s:tt)*] [$($t:tt)*] $n:ident: bytes16(min = $min:expr, max = $max:expr), $($rest:tt)*) => {
        $crate::frames!(@decm $cfg $metas $dec $x $wireop $ver $r
            [$($s)* let $n = $r.take_len16_bytes($min, $max)?;] [$($t)* ($n, &[u8])] $($rest)*);
    };
    (@decm $cfg:tt $metas:tt $dec:ident $x:tt $wireop:tt $ver:tt $r:ident
        [$($s:tt)*] [$($t:tt)*] $n:ident: bytes32(min = $min:expr, max = $max:expr), $($rest:tt)*) => {
        $crate::frames!(@decm $cfg $metas $dec $x $wireop $ver $r
            [$($s)* let $n = $r.take_len32_bytes($min, $max)?;] [$($t)* ($n, &[u8])] $($rest)*);
    };

    // decoder terminal
    (@decm ($m0:expr, $m1:expr, $pv:expr) [$(#[$m:meta])*] $dec:ident [$($x:tt)*] $wireop:tt $ver:tt $r:ident
        [$($s:tt)*] [$($t:tt)*]) => {
        $(#[$m])*
        pub fn $dec($($x)* frame: &[u8]) -> Option<$crate::frames!(@rty $($t)*)> {
            let mut $r = $crate::codec::Reader::new(frame);
            $crate::codec::check_hdr(&mut $r, $m0, $m1, $ver, $wireop)?;
            $($s)*
            $r.finish_exact()?;
            Some($crate::frames!(@rex $($t)*))
        }
    };

    // return type / expression shaping: zero fields → unit, one → bare, many → tuple
    (@rty) => { () };
    (@rty ($n:ident, $t:ty)) => { $t };
    (@rty $(($n:ident, $t:ty))+) => { ($($t),+) };
    (@rex) => { () };
    (@rex ($n:ident, $t:ty)) => { $n };
    (@rex $(($n:ident, $t:ty))+) => { ($($n),+) };
}

#[cfg(test)]
mod tests {
    use crate::codec::testing::assert_reject_matrix;

    /// Synthetic protocol exercising every DSL form — golden bytes below are
    /// the engine's own contract tests (the real protocols bring their own).
    mod testproto {
        pub const MAGIC0: u8 = b'T';
        pub const MAGIC1: u8 = b'P';
        pub const VERSION: u8 = 1;
        pub const OP_ALPHA: u8 = 1;
        pub const OP_BETA: u8 = 2;
        pub const OP_GAMMA: u8 = 3;
        pub const TAG: u8 = 7;

        crate::frames! {
            protocol(magic0 = MAGIC0, magic1 = MAGIC1, version = VERSION);

            /// ALPHA request: scalars, a length-prefixed field, a trailing scalar.
            request encode_alpha / decode_alpha (op = OP_ALPHA) {
                id: u8,
                nonce: u32le,
                name: bytes8(min = 1, max = 48),
                tail: u8,
            }
            /// ALPHA reply: fixed frame with a reserved byte.
            reply fixed encode_alpha_rsp / decode_alpha_rsp (op = OP_ALPHA) {
                status: u8,
                value: u32le,
                _r: pad(1),
            }
            /// BETA request: str + checked literal + str (settingsd shape).
            request encode_beta / decode_beta (op = OP_BETA) {
                key: str8(min = 1, max = 255),
                _t: lit(TAG),
                value: str8(min = 0, max = 255),
            }
            /// Caller-op reply (settingsd response shape).
            reply encode_any_rsp / decode_any_rsp (op = caller) {
                status: u8,
                value: str8(min = 0, max = 255),
            }
            /// One-sided encode with version override + u64 + u16-prefixed bytes.
            request encode encode_gamma (op = OP_GAMMA, version = 2) {
                big: u64le,
                blob: bytes16(min = 0, max = 4096),
            }
            /// One-sided decode of the same frame.
            request decode decode_gamma (op = OP_GAMMA, version = 2) {
                big: u64le,
                blob: bytes16(min = 0, max = 4096),
            }
            /// Fixed encode-only with caller op (policyd response shape).
            reply fixed encode encode_caller_fixed (op = caller) {
                nonce: u32le,
                status: u8,
                _r: pad(1),
            }
            /// Single value field → bare `Option<u8>`, non-zero constrained.
            request encode_single / decode_single (op = 9) {
                n: nz_u8,
            }
            /// u32le-length-prefixed payload.
            request encode_blob32 / decode_blob32 (op = 10) {
                data: bytes32(min = 1, max = 65536),
            }
        }
    }

    use testproto::*;

    #[test]
    fn alpha_golden_and_reject_matrix() {
        let mut buf = [0u8; 64];
        let n = encode_alpha(0xAB, 0x1122_3344, b"svc", 9, &mut buf).unwrap();
        const GOLDEN: [u8; 13] = [
            b'T', b'P', 1, 1,    // hdr
            0xAB, // id
            0x44, 0x33, 0x22, 0x11, // nonce LE
            3, b's', b'v', b'c', // name (last byte `tail` follows)
        ];
        assert_eq!(&buf[..GOLDEN.len()], &GOLDEN);
        assert_eq!(buf[13], 9);
        assert_eq!(decode_alpha(&buf[..n]), Some((0xAB, 0x1122_3344, &b"svc"[..], 9)));
        assert_reject_matrix(&buf[..n], 4, &|f| decode_alpha(f).is_some());
        // Trailing garbage rejected (exact-length rule).
        assert_eq!(decode_alpha(&buf[..n + 1]), None);
    }

    #[test]
    fn alpha_rsp_is_fixed_size_with_reply_bit() {
        // The const-summed return type is the pinned-toolchain proof.
        let frame: [u8; 10] = encode_alpha_rsp(0, 0xA0B0_C0D0);
        assert_eq!(frame, [b'T', b'P', 1, 0x81, 0, 0xD0, 0xC0, 0xB0, 0xA0, 0]);
        assert_eq!(decode_alpha_rsp(&frame), Some((0, 0xA0B0_C0D0)));
        // Padding is skipped, not checked (historical acceptance set).
        let mut padded = frame;
        padded[9] = 0xEE;
        assert_eq!(decode_alpha_rsp(&padded), Some((0, 0xA0B0_C0D0)));
        assert_reject_matrix(&frame, 4, &|f| decode_alpha_rsp(f).is_some());
    }

    #[test]
    fn beta_checks_literal_tag() {
        let mut buf = [0u8; 64];
        let n = encode_beta("k", "vv", &mut buf).unwrap();
        assert_eq!(&buf[..n], &[b'T', b'P', 1, 2, 1, b'k', TAG, 2, b'v', b'v']);
        assert_eq!(decode_beta(&buf[..n]), Some(("k", "vv")));
        // A wrong literal byte must reject (lit is checked on decode).
        let mut bad = buf;
        bad[6] = TAG + 1;
        assert_eq!(decode_beta(&bad[..n]), None);
        // Bounds: empty key refused at encode time.
        assert_eq!(encode_beta("", "v", &mut buf), None);
    }

    #[test]
    fn caller_op_reply_binds_the_reply_bit() {
        let mut buf = [0u8; 32];
        let n = encode_any_rsp(OP_BETA, 0, "ok", &mut buf).unwrap();
        assert_eq!(buf[3], OP_BETA | 0x80);
        assert_eq!(decode_any_rsp(OP_BETA, &buf[..n]), Some((0, "ok")));
        // Same frame checked against the wrong op rejects.
        assert_eq!(decode_any_rsp(OP_ALPHA, &buf[..n]), None);
    }

    #[test]
    fn version_override_and_one_sided_pair() {
        let mut buf = [0u8; 64];
        let n = encode_gamma(0x0102_0304_0506_0708, b"xy", &mut buf).unwrap();
        assert_eq!(&buf[..4], &[b'T', b'P', 2, OP_GAMMA]);
        assert_eq!(&buf[12..14], &[2, 0]); // u16le blob length
        assert_eq!(decode_gamma(&buf[..n]), Some((0x0102_0304_0506_0708, &b"xy"[..])));
        // Protocol-default version must NOT decode an overridden frame.
        let mut v1 = buf;
        v1[2] = 1;
        assert_eq!(decode_gamma(&v1[..n]), None);
    }

    #[test]
    fn fixed_encode_with_caller_op() {
        let frame: [u8; 10] = encode_caller_fixed(5, 0xDEAD_BEEF, 1);
        assert_eq!(frame, [b'T', b'P', 1, 5 | 0x80, 0xEF, 0xBE, 0xAD, 0xDE, 1, 0]);
    }

    #[test]
    fn single_field_returns_bare_value() {
        let mut buf = [0u8; 8];
        let n = encode_single(3, &mut buf).unwrap();
        let got: Option<u8> = decode_single(&buf[..n]);
        assert_eq!(got, Some(3));
        // nz_u8 rejects zero on both sides.
        assert_eq!(encode_single(0, &mut buf), None);
        let zero = [b'T', b'P', 1, 9, 0];
        assert_eq!(decode_single(&zero), None);
    }

    #[test]
    fn blob32_roundtrip_and_bounds() {
        let mut buf = [0u8; 64];
        let n = encode_blob32(b"payload", &mut buf).unwrap();
        assert_eq!(&buf[4..8], &[7, 0, 0, 0]);
        assert_eq!(decode_blob32(&buf[..n]), Some(&b"payload"[..]));
        assert_eq!(encode_blob32(b"", &mut buf), None); // below min
        assert_reject_matrix(&buf[..n], 4, &|f| decode_blob32(f).is_some());
    }
}
