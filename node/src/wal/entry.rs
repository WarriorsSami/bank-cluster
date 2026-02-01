use std::io::Read;
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use bytes::Bytes;

#[derive(Clone, Debug)]
pub struct LogEntry {
    pub index: u64,
    pub term: u64,
    pub command: Bytes,
}

impl LogEntry {
    pub fn encode(&self) -> std::io::Result<Bytes> {
        let mut buf = Vec::new();
        buf.write_u64::<LittleEndian>(self.index)?;
        buf.write_u64::<LittleEndian>(self.term)?;

        let command_len = self.command.len() as u64;
        buf.write_u64::<LittleEndian>(command_len)?;
        buf.extend_from_slice(&self.command);

        Ok(Bytes::from(buf))
    }

    pub fn decode<R: Read>(reader: &mut R) -> std::io::Result<Self> {
        let index = reader.read_u64::<LittleEndian>()?;
        let term = reader.read_u64::<LittleEndian>()?;
        let command_len = reader.read_u64::<LittleEndian>()? as usize;

        let mut command_buf = vec![0u8; command_len];
        reader.read_exact(&mut command_buf)?;

        Ok(LogEntry {
            index,
            term,
            command: Bytes::from(command_buf),
        })
    }
}

#[cfg(test)]
pub(super) mod tests {
    use bytes::Bytes;
    use crate::wal::entry::LogEntry;

    pub(crate) fn create_test_entry(index: u64, term: u64, command: &[u8]) -> LogEntry {
        LogEntry {
            index,
            term,
            command: Bytes::from(command.to_vec()),
        }
    }

    #[test]
    fn test_log_entry_encode_decode_roundtrip() {
        let entry = create_test_entry(42, 3, b"test command");
        let encoded = entry.encode().unwrap();

        let mut cursor = std::io::Cursor::new(encoded.as_ref());
        let decoded = LogEntry::decode(&mut cursor).unwrap();

        assert_eq!(entry.index, decoded.index);
        assert_eq!(entry.term, decoded.term);
        assert_eq!(entry.command, decoded.command);
    }

    #[test]
    fn test_log_entry_encode_decode_empty_command() {
        let entry = create_test_entry(1, 1, b"");
        let encoded = entry.encode().unwrap();

        let mut cursor = std::io::Cursor::new(encoded.as_ref());
        let decoded = LogEntry::decode(&mut cursor).unwrap();

        assert_eq!(entry.index, decoded.index);
        assert_eq!(entry.term, decoded.term);
        assert_eq!(entry.command, decoded.command);
        assert!(decoded.command.is_empty());
    }

    #[test]
    fn test_log_entry_encode_decode_large_command() {
        let large_command = vec![42u8; 10000]; // 10KB command
        let entry = create_test_entry(100, 5, &large_command);
        let encoded = entry.encode().unwrap();

        let mut cursor = std::io::Cursor::new(encoded.as_ref());
        let decoded = LogEntry::decode(&mut cursor).unwrap();

        assert_eq!(entry.index, decoded.index);
        assert_eq!(entry.term, decoded.term);
        assert_eq!(entry.command, decoded.command);
        assert_eq!(decoded.command.len(), 10000);
    }

    #[test]
    fn test_log_entry_decode_incomplete_data() {
        let entry = create_test_entry(1, 1, b"test");
        let encoded = entry.encode().unwrap();

        // Test with truncated data
        let truncated = &encoded[..8]; // Only index bytes
        let mut cursor = std::io::Cursor::new(truncated);
        let result = LogEntry::decode(&mut cursor);
        assert!(result.is_err());
    }

    #[test]
    fn test_log_entry_decode_no_data() {
        let mut cursor = std::io::Cursor::new(&[]);
        let result = LogEntry::decode(&mut cursor);
        assert!(result.is_err());
    }
}
