use nexus_ipc::KernelClient;

use crate::markers::emit_line;

pub(crate) fn verify_vfs() -> Result<(), ()> {
    // RFC-0005: name-based routing (slots are assigned by init-lite; lookup happens over a
    // private control endpoint).
    let _ = KernelClient::new_for("vfsd").map_err(|_| ())?;
    emit_line("SELFTEST: ipc routing ok");
    let _ = KernelClient::new_for("packagefsd").map_err(|_| ())?;
    emit_line("SELFTEST: ipc routing packagefsd ok");

    // Use the nexus-vfs OS backend (no raw opcode frames in the app).
    let vfs = match nexus_vfs::VfsClient::new() {
        Ok(vfs) => vfs,
        Err(_) => {
            emit_line("SELFTEST: vfs client new FAIL");
            return Err(());
        }
    };

    // stat
    let _meta = vfs.stat("pkg:/system/build.prop").map_err(|_| {
        emit_line("SELFTEST: vfs stat FAIL");
    })?;
    emit_line("SELFTEST: vfs stat ok");

    // open
    let fh = vfs.open("pkg:/system/build.prop").map_err(|_| {
        emit_line("SELFTEST: vfs open FAIL");
    })?;

    // read
    let _bytes = vfs.read(fh, 0, 64).map_err(|_| {
        emit_line("SELFTEST: vfs read FAIL");
    })?;
    emit_line("SELFTEST: vfs read ok");

    // real data: deterministic bytes from packagefsd via vfsd
    let fh = vfs.open("pkg:/system/build.prop").map_err(|_| ())?;
    let got = vfs.read(fh, 0, 64).map_err(|_| ())?;
    let expect: &[u8] = b"ro.nexus.build=dev\n";
    if !got.as_slice().starts_with(expect) {
        emit_line("SELFTEST: vfs real data FAIL");
        return Err(());
    }
    emit_line("SELFTEST: vfs real data ok");

    // close
    vfs.close(fh).map_err(|_| ())?;

    // ebadf: read after close should fail
    if vfs.read(fh, 0, 1).is_err() {
        emit_line("SELFTEST: vfs ebadf ok");
        Ok(())
    } else {
        Err(())
    }
}
