# NXB packaging (v1 bootstrap)

The `nxb-pack` host tool prepares a minimal bundle layout while the service
integration is wired. The tool copies an ELF payload and emits a fixed manifest
so QEMU smoke tests can stage assets without talking to bundlemgrd yet.

```
$ cargo run -p nxb-pack -- path/to/app.elf out/demo.hello.nxb
$ tree out/demo.hello.nxb
out/demo.hello.nxb
├── manifest.json
└── payload.elf
```

The manifest is currently hard-coded:

```json
{ "name":"demo.hello", "version":"0.0.1", "required_caps":[], "publisher":"dev", "sig":"" }
```

`payload.elf` is a byte-for-byte copy of the input ELF. Future revisions will
replace this bootstrap packaging with bundlemgrd-provided VMOs once the
bundle→execd hand-off lands (see `docs/services/lifecycle.md`).
