//! Generate the pinned upstream backend for the Orchard worker build.
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    let out = PathBuf::from(env::args_os().nth(1).expect("usage: generate OUT_DIR"));
    fs::create_dir_all(&out).expect("create worker output directory");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let checked = |name: &str, hash: &str| {
        let bytes = fs::read(root.join(name)).unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            hash,
            "gaster source pin: {name}"
        );
        bytes
    };
    let bytes = checked(
        "gaster.c",
        "38521e9232c6e2903ede0adfa358f0723dafd6f0233420106c9b809ca3072ca1",
    );
    let upstream = String::from_utf8(bytes).unwrap();
    // Keep upstream USB/entry and device AES unchanged; omit host image parsing,
    // host crypto, compression and the general-purpose CLI.
    let end = upstream.find("static size_t\ndecompress_lzss(").unwrap();
    let start_aes = upstream.find("static bool\ngaster_command(").unwrap();
    let end_aes = upstream.find("static bool\ngaster_decrypt(").unwrap();
    let mut source = upstream[..end].to_owned() + &upstream[start_aes..end_aes];
    for include in [
        "#include \"lzfse.h\"",
        "#\tinclude <openssl/evp.h>",
        "#\tinclude <CommonCrypto/CommonCrypto.h>",
    ] {
        assert_eq!(source.matches(include).count(), 1);
        source = source.replace(include, "");
    }
    let declarations = "#ifdef HAVE_LIBUSB\nstatic struct libusb_device_handle *open_selected(uint16_t vid, uint16_t pid);\n#else\nstatic bool selected_service(io_service_t service);\n#endif\n";
    source = source.replacen(
        "static uint16_t cpid;",
        &(declarations.to_owned() + "static uint16_t cpid;"),
        1,
    );
    let old = "libusb_open_device_with_vid_pid(NULL, handle->vid, handle->pid)";
    assert_eq!(source.matches(old).count(), 1);
    source = source.replace(old, "open_selected(handle->vid, handle->pid)");
    let old = "if(open_usb_device(serv, handle)) {";
    assert_eq!(source.matches(old).count(), 1);
    source = source.replace(old, "if(!selected_service(serv)) { IOObjectRelease(serv); continue; }\n\t\t\t\tif(open_usb_device(serv, handle)) {");
    // Measurement hooks retain request arguments, return handling and ordering.
    // No trace is written until the worker has finished its USB operations.
    let old = "ret = (*handle->device)->DeviceRequestTO(handle->device, &req);";
    assert_eq!(source.matches(old).count(), 1);
    source = source.replace(old, &format!(
        "timing_record(\"control-start\", 0, bm_request_type, b_request, w_len, 0);\n\t{old}\n\ttiming_record(\"control-end\", ret, bm_request_type, b_request, w_len, req.wLenDone);"
    ));
    for stage in ["reset", "setup", "spray", "patch"] {
        let old = format!("ret = checkm8_stage_{stage}(handle);");
        assert_eq!(source.matches(&old).count(), 1);
        source = source.replace(
            &old,
            &format!(
            "timing_mark(\"{stage}-start\"); {old} timing_record(\"{stage}-end\", ret, 0, 0, 0, 0);"
        ),
        );
    }
    for (name, old) in [
        (
            "reset-device",
            "(*handle->device)->ResetDevice(handle->device);",
        ),
        (
            "reenumerate",
            "(*handle->device)->USBDeviceReEnumerate(handle->device, 0);",
        ),
        (
            "close",
            "(*handle->device)->USBDeviceClose(handle->device);",
        ),
    ] {
        assert!(source.contains(old));
        source = source.replace(old, &format!(
            "timing_mark(\"{name}-start\"); {{ uint32_t timing_result = {old} timing_record(\"{name}-end\", timing_result, 0, 0, 0, 0); }}"
        ));
    }
    fs::write(out.join("device-gaster.c"), source).unwrap();
    for (name, hash) in [
        (
            "payload_A7",
            "0a82ff8b9b18e9b3b3ef380e6484b29d425d39ea1f9dabc92925b58f5d5114ab",
        ),
        (
            "payload_A9",
            "a698045fae09acdc5bfafc26ef3fe7848f1454b8ce8847fdc45396cc18a532b2",
        ),
        (
            "payload_notA9",
            "36fbb1418aa90417fd3ceba88b403d6e3360106b286c5133a54b7a3a4add2190",
        ),
        (
            "payload_notA9_armv7",
            "06d8ec32fe560719ddc90aef49f18fbde2d4ed8dae5e8ac1cab7fce9ef0185e6",
        ),
        (
            "payload_handle_checkm8_request",
            "c0c213047a8902186392396cb7e57c096626b4812b61e07c2cf7cf6b99a9889c",
        ),
        (
            "payload_handle_checkm8_request_armv7",
            "343cf729dd08d693c467304292d26d0ac4132f99ceb29fcb99dd7c8148553a73",
        ),
    ] {
        let bytes = checked(&format!("{name}.bin"), hash);
        let values: String = bytes.iter().map(|b| format!("{b},")).collect();
        fs::write(
            out.join(format!("{name}.h")),
            format!(
                "unsigned char {name}_bin[] = {{{values}}};\nunsigned {name}_bin_len = {};\n",
                bytes.len()
            ),
        )
        .unwrap();
    }
}
