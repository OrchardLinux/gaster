/* Orchard device-key worker. Upstream device code: Apache-2.0, see ../LICENSE.
 * One entry OR one bounded keybag operation per process. No host decryption. */
#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <ctype.h>
#include <time.h>
#include <unistd.h>
#include <errno.h>
/* Optional measurements stay in memory until checkm8 has finished. No USB
 * data or key material is recorded, only request metadata and return codes. */
struct timing { const char *name; uint64_t ns; uint32_t result, length, actual; unsigned type, request; };
static struct timing timings[8192];
static size_t timing_count, timing_dropped;
static const char *timing_path;
static uint64_t timing_origin;
static uint64_t timing_now(void) {
    struct timespec t;
    if (clock_gettime(CLOCK_MONOTONIC, &t)) return 0;
    return (uint64_t)t.tv_sec * 1000000000 + t.tv_nsec;
}
static void timing_record(const char *name, uint32_t result, unsigned type,
                          unsigned request, uint32_t length, uint32_t actual) {
    if (!timing_path) return;
    uint64_t now = timing_now();
    if (timing_count == sizeof(timings) / sizeof(timings[0])) { timing_dropped++; return; }
    timings[timing_count++] = (struct timing){name, now - timing_origin, result, length, actual, type, request};
}
static void timing_mark(const char *name) { timing_record(name, 0, 0, 0, 0, 0); }
static void timing_flush(void) {
    if (!timing_path) return;
    FILE *out = fopen(timing_path, "wx");
    if (!out) return;
    fprintf(out, "# dropped=%zu; usb_timeout_ms=5\nseconds,event,result,type,request,length,actual\n", timing_dropped);
    for (size_t i = 0; i < timing_count; i++) {
        struct timing *t = &timings[i];
        fprintf(out, "%.9f,%s,0x%08x,0x%02x,%u,%u,%u\n", t->ns / 1e9, t->name,
                t->result, t->type, t->request, t->length, t->actual);
    }
    fclose(out);
}
#include "device-gaster.c"

static uint64_t selected_ecid;
static uint32_t selected_chip, selected_board;
static bool unwrap_only;

static bool field(const char *serial, const char *name, uint64_t *value) {
    size_t n = strlen(name);
    for (const char *p = serial; p && *p; p = strchr(p, ' ')) {
        while (*p == ' ') ++p;
        if (strncmp(p, name, n) || p[n] != ':') continue;
        char *end;
        if (!isxdigit((unsigned char)p[n + 1])) return false;
        errno = 0;
        *value = strtoull(p + n + 1, &end, 16);
        return !errno && (*end == 0 || *end == ' ');
    }
    return false;
}
static bool matches(const char *serial) {
    uint64_t e, c, b;
    if (!serial || !field(serial, "ECID", &e) || e != selected_ecid ||
        !field(serial, "CPID", &c) || c != selected_chip ||
        !field(serial, "BDID", &b) || b != selected_board) return false;
    const char *p = strstr(serial, " PWND:[");
    bool ours = p && !strncmp(p, " PWND:[gaster]", 14) && (!p[14] || p[14] == ' ');
    return (!p || ours) && (!unwrap_only || ours);
}
#ifdef HAVE_LIBUSB
static struct libusb_device_handle *open_selected(uint16_t vid, uint16_t pid) {
    struct libusb_device **devices = NULL;
    struct libusb_device_handle *selected = NULL;
    ssize_t count = libusb_get_device_list(NULL, &devices);
    for (ssize_t i = 0; i < count; ++i) {
        struct libusb_device_descriptor descriptor;
        usb_handle_t candidate;
        if (libusb_get_device_descriptor(devices[i], &descriptor) ||
            descriptor.idVendor != vid || descriptor.idProduct != pid ||
            libusb_open(devices[i], &candidate.device)) continue;
        char *serial = get_usb_serial_number(&candidate);
        bool ok = matches(serial);
        free(serial);
        if (ok) { selected = candidate.device; break; }
        libusb_close(candidate.device);
    }
    if (devices) libusb_free_device_list(devices, 1);
    return selected;
}
#else
static bool selected_service(io_service_t service) {
    char serial[1024];
    CFTypeRef value = IORegistryEntryCreateCFProperty(service, CFSTR(kUSBSerialNumberString), kCFAllocatorDefault, 0);
    bool ok = value && CFGetTypeID(value) == CFStringGetTypeID() &&
        CFStringGetCString((CFStringRef)value, serial, sizeof(serial), kCFStringEncodingUTF8) && matches(serial);
    if (value) CFRelease(value);
    return ok;
}
#endif
static bool number(const char *text, uint64_t *n) {
    if (!text || !*text || !isxdigit((unsigned char)*text)) return false;
    char *end;
    errno = 0;
    *n = strtoull(text, &end, 16);
    return !errno && !*end;
}
static int self_test(void) {
    timing_mark("disabled");
    if (timing_count) return 1;
    timing_path = "self-test";
    timing_origin = timing_now();
    if (!timing_origin) return 1;
    timing_record("test", 0x1234, 0x21, 1, 16, 8);
    if (timing_count != 1 || timings[0].result != 0x1234 ||
        timings[0].length != 16 || timings[0].actual != 8) return 1;
    for (size_t i = 0; i < 8192; i++) timing_mark("fill");
    if (timing_count != 8192 || timing_dropped != 1) return 1;
    timing_path = NULL;
    timing_count = timing_dropped = 0;
    selected_ecid = 0xa1; selected_chip = 0x8010; selected_board = 0x0c;
    unwrap_only = true;
    if (!matches("CPID:8010 BDID:0C ECID:A1 PWND:[gaster]")) return 1;
    if (matches("CPID:8010 BDID:0C ECID:B2 PWND:[gaster]") ||
        matches("CPID:8010 BDID:0C ECID:A1 PWND:[gaster]suffix") ||
        matches("CPID:8010 BDID:0C ECID:A1 PWND:[checkm8]") ||
        matches("CPID:8010 BDID:0C ECID:A1")) return 1;
    unwrap_only = false;
    if (!matches("CPID:8010 BDID:0C ECID:A1") ||
        matches("CPID:8010 BDID:0C ECID:A1 PWND:[usbliter8]")) return 1;
    selected_ecid = 0xb2; selected_chip = 0x8000; selected_board = 4;
    return matches("CPID:8000 BDID:04 ECID:B2") &&
        !matches("CPID:8010 BDID:0C ECID:A1") ? 0 : 1;
}
int main(int argc, char **argv) {
    if (argc == 2 && !strcmp(argv[1], "--self-test")) return self_test();
    /* All validation precedes even USB enumeration. Reply stdout is duplicated
     * before upstream's diagnostics are redirected: to stderr for an entry,
     * which handles no secret, so the caller can log each stage as it is
     * tried; to /dev/null for a key operation. */
    uint64_t chip, board, seconds;
    if (argc != 7 || (strcmp(argv[1], "pwn") && strcmp(argv[1], "unwrap")) ||
        !number(argv[2], &selected_ecid) || !selected_ecid ||
        !number(argv[3], &chip) || chip > UINT32_MAX ||
        !number(argv[4], &board) || board > UINT32_MAX ||
        !number(argv[5], &seconds) || !seconds || seconds > 3600) return 2;
    selected_chip = (uint32_t)chip; selected_board = (uint32_t)board;
    unwrap_only = !strcmp(argv[1], "unwrap");
    uint8_t bags[144], keys[144];
    size_t size = 0;
    if (unwrap_only) {
        FILE *input = fopen(argv[6], "rb");
        if (!input) return 2;
        size = fread(bags, 1, sizeof(bags), input);
        bool ok = size && size % 48 == 0 && fgetc(input) == EOF && !ferror(input);
        fclose(input);
        if (!ok) return 2;
    } else if (strcmp(argv[6], "-")) return 2;
    int reply = dup(STDOUT_FILENO);
    if (reply < 0 || !freopen("/dev/null", "w", stdout)) return 2;
    if (!unwrap_only) {
        if (dup2(STDERR_FILENO, STDOUT_FILENO) < 0) return 2;
        setvbuf(stdout, NULL, _IOLBF, 0);
    }
    alarm((unsigned)seconds);
    usb_timeout = 5; usb_abort_timeout_min = 0;
    usb_handle_t handle;
    if (!unwrap_only) {
        timing_path = getenv("ORCHARD_GASTER_TRACE");
        timing_origin = timing_now();
        timing_mark("worker-start");
        bool ok = gaster_checkm8(&handle);
        timing_record("worker-end", ok, 0, 0, 0, 0);
        timing_flush();
        return ok ? 0 : 1;
    }
    /* Descriptor inspection initializes the ROM AES addresses. There is no
     * path from here to checkm8 or its reset/exploit stages. */
    bool pwned = false;
    init_usb_handle(&handle, APPLE_VID, DFU_MODE_PID);
    if (!wait_usb_handle(&handle, checkm8_check_usb_device, &pwned)) return 1;
    close_usb_handle(&handle);
    if (!pwned || cpid != selected_chip) return 1;
    for (size_t i = 0; i < size; i += 48) {
        if (!gaster_aes(&handle, AES_CMD_CBC | AES_CMD_DEC, bags + i, keys + i, 48,
                        AES_KEY_SZ_256 | AES_KEY_TYPE_GID0)) return 1;
    }
    ssize_t written = write(reply, keys, size);
    volatile uint8_t *clear = keys;
    for (size_t i = 0; i < sizeof(keys); ++i) clear[i] = 0;
    return written == (ssize_t)size ? 0 : 1;
}
