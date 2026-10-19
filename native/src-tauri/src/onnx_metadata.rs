//! Read ModelProto.metadata_props without loading tensors or invoking native code.
//! ONNX stores metadata as protobuf field 14 (StringStringEntryProto entries).
use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::Path,
};

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "Invalid ONNX metadata")
}

fn varint(reader: &mut impl Read) -> io::Result<u64> {
    let mut value = 0;
    for shift in (0..70).step_by(7) {
        let mut byte = [0];
        reader.read_exact(&mut byte)?;
        if shift == 63 && byte[0] > 1 {
            return Err(invalid());
        }
        value |= u64::from(byte[0] & 0x7f) << shift;
        if byte[0] & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(invalid())
}

/// Return the next field's number and length-delimited contents, skipping other
/// wire types. Every seek is bounded by the enclosing message, even at EOF.
fn field(reader: &mut (impl Read + Seek), end: u64) -> io::Result<Option<(u64, u64)>> {
    while reader.stream_position()? < end {
        let tag = varint(reader)?;
        if tag >> 3 == 0 {
            return Err(invalid());
        }
        let length = match tag & 7 {
            0 => {
                varint(reader)?;
                if reader.stream_position()? > end {
                    return Err(invalid());
                }
                continue;
            }
            1 => 8,
            2 => varint(reader)?,
            5 => 4,
            _ => return Err(invalid()),
        };
        let next = reader
            .stream_position()?
            .checked_add(length)
            .ok_or_else(invalid)?;
        if next > end {
            return Err(invalid());
        }
        if tag & 7 == 2 {
            return Ok(Some((tag >> 3, next)));
        }
        reader.seek(SeekFrom::Start(next))?;
    }
    Ok(None)
}

pub fn read(path: &Path) -> io::Result<BTreeMap<String, String>> {
    let mut reader = File::open(path)?;
    let end = reader.metadata()?.len();
    let mut metadata = BTreeMap::new();
    while let Some((number, next)) = field(&mut reader, end)? {
        if number == 14 {
            // Metadata strings are small; never allocate based on tensor sizes
            // or untrusted lengths from a damaged model.
            if next - reader.stream_position()? > 64 * 1024 {
                return Err(invalid());
            }
            let (mut key, mut value) = (None, None);
            while let Some((number, entry_end)) = field(&mut reader, next)? {
                if number == 1 || number == 2 {
                    let length = entry_end - reader.stream_position()?;
                    let mut bytes = vec![0; length as usize];
                    reader.read_exact(&mut bytes)?;
                    let text = String::from_utf8(bytes).map_err(|_| invalid())?;
                    if number == 1 {
                        key = Some(text);
                    } else {
                        value = Some(text);
                    }
                }
                reader.seek(SeekFrom::Start(entry_end))?;
            }
            let key = key.ok_or_else(invalid)?;
            if metadata.insert(key, value.ok_or_else(invalid)?).is_some() {
                return Err(invalid());
            }
        }
        reader.seek(SeekFrom::Start(next))?;
    }
    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_bytes(bytes: &[u8]) -> io::Result<BTreeMap<String, String>> {
        let path = std::env::temp_dir().join(format!("golos-{}.onnx", uuid::Uuid::new_v4()));
        std::fs::write(&path, bytes).unwrap();
        let result = read(&path);
        std::fs::remove_file(path).unwrap();
        result
    }

    #[test]
    fn reads_metadata_and_skips_graph_and_other_wire_types() {
        // IR version, graph (with a metadata-looking byte sequence inside),
        // fixed32, fixed64, then the actual framework metadata entry.
        let mut bytes = vec![8, 9, 58, 2, 114, 0, 29, 0, 0, 0, 0, 33];
        bytes.extend([0; 8]);
        bytes.extend(b"\x72\x11\x0a\x09framework\x12\x04nemo");
        assert_eq!(read_bytes(&bytes).unwrap()["framework"], "nemo");
    }

    #[test]
    fn rejects_truncated_overflowing_and_ambiguous_metadata() {
        let entry = b"\x72\x11\x0a\x09framework\x12\x04nemo";
        let mut duplicate = entry.to_vec();
        duplicate.extend(entry);
        for bytes in [
            &b"\x72\xff"[..],
            &entry[..entry.len() - 1],
            &b"\x72\x02\x0a\x7f"[..],
            &b"\x00"[..],
            &b"\x08\xff\xff\xff\xff\xff\xff\xff\xff\xff\x02"[..],
            &duplicate,
        ] {
            assert!(read_bytes(bytes).is_err(), "accepted {bytes:?}");
        }
    }
}
