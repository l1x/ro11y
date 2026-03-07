/// Wire types per protobuf spec.
const WIRE_TYPE_VARINT: u8 = 0;
const WIRE_TYPE_FIXED64: u8 = 1;
const WIRE_TYPE_LENGTH_DELIMITED: u8 = 2;

/// Encode a varint (LEB128) into the buffer.
fn encode_varint(buf: &mut Vec<u8>, mut val: u64) {
    while val >= 0x80 {
        buf.push((val as u8) | 0x80);
        val >>= 7;
    }
    buf.push(val as u8);
}

/// Encode a field tag (field number + wire type).
fn encode_tag(buf: &mut Vec<u8>, field_number: u32, wire_type: u8) {
    encode_varint(buf, ((field_number as u64) << 3) | wire_type as u64);
}

/// Encode a varint field (tag + varint value).
/// Follows proto3 convention: skips the field if val == 0.
pub(crate) fn encode_varint_field(buf: &mut Vec<u8>, field: u32, val: u64) {
    if val == 0 {
        return;
    }
    encode_tag(buf, field, WIRE_TYPE_VARINT);
    encode_varint(buf, val);
}

/// Encode a varint field unconditionally, even if val == 0.
/// Use for fields where zero is a meaningful value (e.g. bool false, int 0).
pub(crate) fn encode_varint_field_always(buf: &mut Vec<u8>, field: u32, val: u64) {
    encode_tag(buf, field, WIRE_TYPE_VARINT);
    encode_varint(buf, val);
}

/// Encode a string field (tag + length + UTF-8 bytes).
/// Skips the field if the string is empty (proto3 default).
pub(crate) fn encode_string_field(buf: &mut Vec<u8>, field: u32, s: &str) {
    if s.is_empty() {
        return;
    }
    encode_bytes_field(buf, field, s.as_bytes());
}

/// Encode a bytes field (tag + length + raw bytes).
/// Skips the field if the slice is empty (proto3 default).
pub(crate) fn encode_bytes_field(buf: &mut Vec<u8>, field: u32, data: &[u8]) {
    if data.is_empty() {
        return;
    }
    encode_tag(buf, field, WIRE_TYPE_LENGTH_DELIMITED);
    encode_varint(buf, data.len() as u64);
    buf.extend_from_slice(data);
}

/// Encode a fixed64 field (tag + 8 bytes little-endian).
/// Skips the field if val == 0 (proto3 default).
pub(crate) fn encode_fixed64_field(buf: &mut Vec<u8>, field: u32, val: u64) {
    if val == 0 {
        return;
    }
    encode_tag(buf, field, WIRE_TYPE_FIXED64);
    buf.extend_from_slice(&val.to_le_bytes());
}

/// Encode a fixed64 field unconditionally, even if val == 0.
pub(crate) fn encode_fixed64_field_always(buf: &mut Vec<u8>, field: u32, val: u64) {
    encode_tag(buf, field, WIRE_TYPE_FIXED64);
    buf.extend_from_slice(&val.to_le_bytes());
}

/// Encode a nested message field (tag + length + message bytes).
/// Skips the field if the message is empty.
#[cfg(test)]
pub(crate) fn encode_message_field(buf: &mut Vec<u8>, field: u32, msg: &[u8]) {
    if msg.is_empty() {
        return;
    }
    encode_tag(buf, field, WIRE_TYPE_LENGTH_DELIMITED);
    encode_varint(buf, msg.len() as u64);
    buf.extend_from_slice(msg);
}

/// Encode a nested message field in-place using a closure.
///
/// Instead of allocating a temporary `Vec<u8>` for the message body,
/// this writes the tag, reserves space for the length, lets the closure
/// write the body directly into `buf`, then patches the length.
///
/// Skips the field entirely if the closure produces an empty body.
pub fn encode_message_field_in_place<F>(buf: &mut Vec<u8>, field: u32, f: F)
where
    F: FnOnce(&mut Vec<u8>),
{
    // Write tag
    encode_tag(buf, field, WIRE_TYPE_LENGTH_DELIMITED);
    let tag_end = buf.len();

    // Reserve 5 bytes for the length (max varint32 size)
    const MAX_LEN_BYTES: usize = 5;
    buf.extend_from_slice(&[0u8; MAX_LEN_BYTES]);
    let body_start = buf.len();

    // Let the closure write the body
    f(buf);

    let body_len = buf.len() - body_start;

    if body_len == 0 {
        // Empty body — remove the tag + reserved length bytes
        buf.truncate(tag_end - varint_tag_len(field));
        return;
    }

    // Encode the actual length as a varint into a small stack buffer
    let mut len_buf = [0u8; MAX_LEN_BYTES];
    let varint_len = encode_varint_to_slice(&mut len_buf, body_len as u64);

    if varint_len < MAX_LEN_BYTES {
        // Copy the varint into the reserved slot
        buf.copy_within(body_start..body_start + body_len, tag_end + varint_len);
        buf[tag_end..tag_end + varint_len].copy_from_slice(&len_buf[..varint_len]);
        buf.truncate(tag_end + varint_len + body_len);
    } else {
        // Exactly 5 bytes — just overwrite in place
        buf[tag_end..tag_end + MAX_LEN_BYTES].copy_from_slice(&len_buf);
    }
}

/// Encode a varint into a fixed-size slice, returning the number of bytes written.
fn encode_varint_to_slice(out: &mut [u8; 5], mut val: u64) -> usize {
    let mut i = 0;
    while val >= 0x80 {
        out[i] = (val as u8) | 0x80;
        val >>= 7;
        i += 1;
    }
    out[i] = val as u8;
    i + 1
}

/// Return the number of bytes a varint-encoded tag occupies.
fn varint_tag_len(field: u32) -> usize {
    let tag_val = (field as u64) << 3 | WIRE_TYPE_LENGTH_DELIMITED as u64;
    let mut v = tag_val;
    let mut n = 1;
    while v >= 0x80 {
        v >>= 7;
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_zero() {
        let mut buf = Vec::new();
        encode_varint(&mut buf, 0);
        assert_eq!(buf, vec![0x00]);
    }

    #[test]
    fn varint_one() {
        let mut buf = Vec::new();
        encode_varint(&mut buf, 1);
        assert_eq!(buf, vec![0x01]);
    }

    #[test]
    fn varint_127() {
        let mut buf = Vec::new();
        encode_varint(&mut buf, 127);
        assert_eq!(buf, vec![0x7F]);
    }

    #[test]
    fn varint_128() {
        let mut buf = Vec::new();
        encode_varint(&mut buf, 128);
        assert_eq!(buf, vec![0x80, 0x01]);
    }

    #[test]
    fn varint_300() {
        let mut buf = Vec::new();
        encode_varint(&mut buf, 300);
        assert_eq!(buf, vec![0xAC, 0x02]);
    }

    #[test]
    fn string_field_encoding() {
        let mut buf = Vec::new();
        encode_string_field(&mut buf, 1, "hi");
        assert_eq!(buf, vec![0x0A, 0x02, b'h', b'i']);
    }

    #[test]
    fn string_field_empty_is_skipped() {
        let mut buf = Vec::new();
        encode_string_field(&mut buf, 1, "");
        assert!(buf.is_empty());
    }

    #[test]
    fn fixed64_encoding() {
        let mut buf = Vec::new();
        encode_fixed64_field(&mut buf, 1, 0x0102030405060708);
        assert_eq!(
            buf,
            vec![0x09, 0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01]
        );
    }

    #[test]
    fn fixed64_zero_is_skipped() {
        let mut buf = Vec::new();
        encode_fixed64_field(&mut buf, 1, 0);
        assert!(buf.is_empty());
    }

    #[test]
    fn fixed64_always_encodes_zero() {
        let mut buf = Vec::new();
        encode_fixed64_field_always(&mut buf, 1, 0);
        assert_eq!(buf, vec![0x09, 0, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn varint_always_encodes_zero() {
        let mut buf = Vec::new();
        encode_varint_field_always(&mut buf, 2, 0);
        // tag = (2<<3)|0 = 0x10, value = 0x00
        assert_eq!(buf, vec![0x10, 0x00]);
    }

    #[test]
    fn nested_message_encoding() {
        let mut inner = Vec::new();
        encode_string_field(&mut inner, 1, "ab");

        let mut buf = Vec::new();
        encode_message_field(&mut buf, 2, &inner);

        assert_eq!(buf, vec![0x12, 0x04, 0x0A, 0x02, b'a', b'b']);
    }

    #[test]
    fn varint_field_encoding() {
        let mut buf = Vec::new();
        encode_varint_field(&mut buf, 3, 150);
        assert_eq!(buf, vec![0x18, 0x96, 0x01]);
    }

    #[test]
    fn varint_field_zero_is_skipped() {
        let mut buf = Vec::new();
        encode_varint_field(&mut buf, 3, 0);
        assert!(buf.is_empty());
    }

    #[test]
    fn bytes_field_encoding() {
        let mut buf = Vec::new();
        encode_bytes_field(&mut buf, 1, &[0xDE, 0xAD]);
        assert_eq!(buf, vec![0x0A, 0x02, 0xDE, 0xAD]);
    }

    #[test]
    fn encode_message_field_in_place_matches_original() {
        // Build with original approach
        let mut inner = Vec::new();
        encode_string_field(&mut inner, 1, "ab");
        let mut expected = Vec::new();
        encode_message_field(&mut expected, 2, &inner);

        // Build with in-place approach
        let mut actual = Vec::new();
        encode_message_field_in_place(&mut actual, 2, |buf| {
            encode_string_field(buf, 1, "ab");
        });

        assert_eq!(actual, expected);
    }

    #[test]
    fn encode_message_field_in_place_empty_body_is_skipped() {
        let mut buf = Vec::new();
        encode_message_field_in_place(&mut buf, 2, |_buf| {
            // write nothing
        });
        assert!(buf.is_empty());
    }

    #[test]
    fn encode_message_field_in_place_nested() {
        // Nested in-place: outer message containing inner message
        let mut expected_inner = Vec::new();
        encode_string_field(&mut expected_inner, 1, "hello");
        let mut expected_mid = Vec::new();
        encode_message_field(&mut expected_mid, 1, &expected_inner);
        let mut expected = Vec::new();
        encode_message_field(&mut expected, 2, &expected_mid);

        let mut actual = Vec::new();
        encode_message_field_in_place(&mut actual, 2, |buf| {
            encode_message_field_in_place(buf, 1, |buf| {
                encode_string_field(buf, 1, "hello");
            });
        });

        assert_eq!(actual, expected);
    }

    #[test]
    fn encode_message_field_in_place_large_body() {
        // Body > 127 bytes requires multi-byte varint length
        let large_data = vec![0x42u8; 200];

        let mut expected = Vec::new();
        encode_message_field(&mut expected, 1, &large_data);

        let mut actual = Vec::new();
        encode_message_field_in_place(&mut actual, 1, |buf| {
            buf.extend_from_slice(&large_data);
        });

        assert_eq!(actual, expected);
    }
}
