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
    // The setup stage is a race the host has to win: a transfer aborted after
    // only part of its data went out. The requests and their order stay as
    // upstream's; added are a count of what each attempt sent, for the
    // caller's log, an end when the device no longer takes requests (upstream
    // tries for ever), and a limit of its own for the wait before the abort.
    let old = "\tfor(;;) {\n\t\tif(send_usb_control_request_async_no_data(handle, 0x21, DFU_DNLOAD, 0, 0, DFU_MAX_TRANSFER_SZ, usb_abort_timeout, &transfer_ret) && transfer_ret.sz < config_overwrite_pad && send_usb_control_request_no_data(handle, 0, 0, 0, 0, config_overwrite_pad - transfer_ret.sz, &transfer_ret) && transfer_ret.ret == USB_TRANSFER_STALL) {\n\t\t\treturn true;\n\t\t}\n\t\tsend_usb_control_request_no_data(handle, 0x21, DFU_DNLOAD, 0, 0, EP0_MAX_PACKET_SZ, NULL);\n\t\tusb_abort_timeout = (usb_abort_timeout + 1) % (usb_timeout - usb_abort_timeout_min + 1) + usb_abort_timeout_min;\n\t}";
    assert_eq!(source.matches(old).count(), 1);
    source = source.replace(old, "\tusb_abort_timeout = setup_first_abort();\n\tfor(;;) {\n\t\tbool sent = send_usb_control_request_async_no_data(handle, 0x21, DFU_DNLOAD, 0, 0, DFU_MAX_TRANSFER_SZ, usb_abort_timeout, &transfer_ret);\n\t\tif(!setup_attempt(sent, sent ? transfer_ret.sz : 0, config_overwrite_pad)) {\n\t\t\treturn false;\n\t\t}\n\t\tif(sent && transfer_ret.sz < config_overwrite_pad && send_usb_control_request_no_data(handle, 0, 0, 0, 0, config_overwrite_pad - transfer_ret.sz, &transfer_ret) && transfer_ret.ret == USB_TRANSFER_STALL) {\n\t\t\tsetup_report(\"won\", config_overwrite_pad);\n\t\t\treturn true;\n\t\t}\n\t\tsend_usb_control_request_no_data(handle, 0x21, DFU_DNLOAD, 0, 0, EP0_MAX_PACKET_SZ, NULL);\n\t\tusb_abort_timeout = setup_next_abort(usb_abort_timeout);\n\t}");
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
