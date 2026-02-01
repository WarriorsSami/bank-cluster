use std::io::{Seek, Write};
use crate::wal::entry::LogEntry;

#[derive(Debug)]
pub struct Wal {
    file: std::fs::File,
    last_index: u64,
}

impl Wal {
    pub fn new(path: &str) -> std::io::Result<Self> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(path)?;

        let last_index = Self::scan_last_index(&file)?;

        Ok(Self { file, last_index })
    }

    fn scan_last_index(file: &std::fs::File) -> std::io::Result<u64> {
        let mut file = file.try_clone()?;
        file.seek(std::io::SeekFrom::Start(0))?;

        let mut reader = std::io::BufReader::new(file);
        let mut last_index = 0;

        loop {
            match LogEntry::decode(&mut reader) {
                Ok(entry) => {
                    if entry.index != last_index + 1 {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "Log entries are not sequential",
                        ));
                    }
                    last_index = entry.index;
                }
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e),
            }
        }

        Ok(last_index)
    }

    pub fn append(&mut self, entry: LogEntry) -> std::io::Result<()> {
        let encoded = entry.encode()?;

        self.file.write_all(&encoded)?;
        self.file.sync_data()?;

        self.last_index = entry.index;
        Ok(())
    }

    pub fn replay(&self) -> std::io::Result<Vec<LogEntry>> {
        let mut file = self.file.try_clone()?;
        file.seek(std::io::SeekFrom::Start(0))?;

        let mut reader = std::io::BufReader::new(file);
        let mut entries = Vec::new();

        loop {
            match LogEntry::decode(&mut reader) {
                Ok(entry) => entries.push(entry),
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e),
            }
        }

        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use std::fs;
    use std::io::Write;
    use tempfile::{NamedTempFile, TempDir};
    use crate::wal::entry::tests::create_test_entry;

    #[test]
    fn test_wal_creation_new_file() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_str().unwrap();

        let wal = Wal::new(path).unwrap();
        assert_eq!(wal.last_index, 0);
    }

    #[test]
    fn test_wal_creation_existing_empty_file() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_str().unwrap();

        // Create empty file
        fs::write(path, b"").unwrap();

        let wal = Wal::new(path).unwrap();
        assert_eq!(wal.last_index, 0);
    }

    #[test]
    fn test_wal_append_single_entry() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_str().unwrap();

        let mut wal = Wal::new(path).unwrap();
        let entry = create_test_entry(1, 1, b"first entry");

        wal.append(entry.clone()).unwrap();
        assert_eq!(wal.last_index, 1);
    }

    #[test]
    fn test_wal_append_multiple_entries() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_str().unwrap();

        let mut wal = Wal::new(path).unwrap();

        for i in 1..=5 {
            let entry = create_test_entry(i, 1, format!("entry {}", i).as_bytes());
            wal.append(entry).unwrap();
        }

        assert_eq!(wal.last_index, 5);
    }

    #[test]
    fn test_wal_append_entries_different_terms() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_str().unwrap();

        let mut wal = Wal::new(path).unwrap();

        // Append entries with increasing terms
        for i in 1..=3 {
            let entry = create_test_entry(i, i, format!("entry {} term {}", i, i).as_bytes());
            wal.append(entry).unwrap();
        }

        assert_eq!(wal.last_index, 3);
    }

    #[test]
    fn test_wal_replay_empty() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_str().unwrap();

        let wal = Wal::new(path).unwrap();
        let entries = wal.replay().unwrap();

        assert!(entries.is_empty());
    }

    #[test]
    fn test_wal_replay_single_entry() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_str().unwrap();

        let mut wal = Wal::new(path).unwrap();
        let original_entry = create_test_entry(1, 1, b"test entry");

        wal.append(original_entry.clone()).unwrap();

        let replayed_entries = wal.replay().unwrap();
        assert_eq!(replayed_entries.len(), 1);

        let replayed_entry = &replayed_entries[0];
        assert_eq!(replayed_entry.index, original_entry.index);
        assert_eq!(replayed_entry.term, original_entry.term);
        assert_eq!(replayed_entry.command, original_entry.command);
    }

    #[test]
    fn test_wal_replay_multiple_entries() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_str().unwrap();

        let mut wal = Wal::new(path).unwrap();
        let mut original_entries = Vec::new();

        for i in 1..=5 {
            let entry = create_test_entry(i, i, format!("entry {}", i).as_bytes());
            original_entries.push(entry.clone());
            wal.append(entry).unwrap();
        }

        let replayed_entries = wal.replay().unwrap();
        assert_eq!(replayed_entries.len(), 5);

        for (original, replayed) in original_entries.iter().zip(replayed_entries.iter()) {
            assert_eq!(original.index, replayed.index);
            assert_eq!(original.term, replayed.term);
            assert_eq!(original.command, replayed.command);
        }
    }

    #[test]
    fn test_wal_persistence_across_instances() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_str().unwrap();

        // Create WAL and append entries
        {
            let mut wal = Wal::new(path).unwrap();
            for i in 1..=3 {
                let entry = create_test_entry(i, 1, format!("persistent entry {}", i).as_bytes());
                wal.append(entry).unwrap();
            }
        }

        // Create new WAL instance and verify persistence
        {
            let wal = Wal::new(path).unwrap();
            assert_eq!(wal.last_index, 3);

            let entries = wal.replay().unwrap();
            assert_eq!(entries.len(), 3);
            assert_eq!(entries[2].command, Bytes::from("persistent entry 3"));
        }
    }

    #[test]
    fn test_wal_scan_last_index_empty() {
        let temp_file = NamedTempFile::new().unwrap();
        let file = fs::File::open(temp_file.path()).unwrap();

        let last_index = Wal::scan_last_index(&file).unwrap();
        assert_eq!(last_index, 0);
    }

    #[test]
    fn test_wal_scan_last_index_with_entries() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_str().unwrap();

        // Write entries directly to file
        {
            let mut file = fs::File::create(path).unwrap();
            for i in 1..=3 {
                let entry = create_test_entry(i, 1, b"test");
                let encoded = entry.encode().unwrap();
                file.write_all(&encoded).unwrap();
            }
        }

        let file = fs::File::open(path).unwrap();
        let last_index = Wal::scan_last_index(&file).unwrap();
        assert_eq!(last_index, 3);
    }

    #[test]
    #[should_panic(expected = "Log entries are not sequential")]
    fn test_wal_scan_last_index_non_sequential_entries() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_str().unwrap();

        // Write non-sequential entries
        {
            let mut file = fs::File::create(path).unwrap();
            let entry1 = create_test_entry(1, 1, b"test");
            let entry3 = create_test_entry(3, 1, b"test"); // Skip index 2

            file.write_all(&entry1.encode().unwrap()).unwrap();
            file.write_all(&entry3.encode().unwrap()).unwrap();
        }

        let file = fs::File::open(path).unwrap();
        Wal::scan_last_index(&file).unwrap();
    }

    #[test]
    fn test_wal_append_after_restart() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_str().unwrap();

        // First session: append some entries
        {
            let mut wal = Wal::new(path).unwrap();
            for i in 1..=3 {
                let entry = create_test_entry(i, 1, format!("session1 entry {}", i).as_bytes());
                wal.append(entry).unwrap();
            }
        }

        // Second session: continue appending
        {
            let mut wal = Wal::new(path).unwrap();
            assert_eq!(wal.last_index, 3);

            for i in 4..=6 {
                let entry = create_test_entry(i, 2, format!("session2 entry {}", i).as_bytes());
                wal.append(entry).unwrap();
            }

            assert_eq!(wal.last_index, 6);
        }

        // Third session: verify all entries
        {
            let wal = Wal::new(path).unwrap();
            let entries = wal.replay().unwrap();
            assert_eq!(entries.len(), 6);
            assert_eq!(wal.last_index, 6);
        }
    }

    #[test]
    fn test_wal_large_entries() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_str().unwrap();

        let mut wal = Wal::new(path).unwrap();

        // Create entries with progressively larger commands
        let sizes = vec![100, 1000, 10000, 100000];
        for (i, size) in sizes.iter().enumerate() {
            let large_command = vec![(i + 1) as u8; *size];
            let entry = create_test_entry((i + 1) as u64, 1, &large_command);
            wal.append(entry).unwrap();
        }

        // Verify all entries can be replayed correctly
        let entries = wal.replay().unwrap();
        assert_eq!(entries.len(), 4);

        for (i, (entry, expected_size)) in entries.iter().zip(sizes.iter()).enumerate() {
            assert_eq!(entry.index, (i + 1) as u64);
            assert_eq!(entry.command.len(), *expected_size);
            assert!(entry.command.iter().all(|&b| b == ((i + 1) as u8)));
        }
    }

    #[test]
    fn test_wal_edge_cases() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_str().unwrap();

        let mut wal = Wal::new(path).unwrap();

        // Test with maximum u64 values
        let entry = LogEntry {
            index: u64::MAX,
            term: u64::MAX,
            command: Bytes::from(vec![255u8; 100]),
        };

        wal.append(entry.clone()).unwrap();

        let entries = wal.replay().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].index, u64::MAX);
        assert_eq!(entries[0].term, u64::MAX);
        assert_eq!(entries[0].command.len(), 100);
    }

    #[test]
    fn test_wal_invalid_file_path() {
        let result = Wal::new("/invalid/path/that/does/not/exist/wal.log");
        assert!(result.is_err());
    }

    #[test]
    fn test_wal_file_permissions() {
        let temp_dir = TempDir::new().unwrap();
        let temp_file = temp_dir.path().join("wal.log");
        let path = temp_file.to_str().unwrap();

        // Create WAL and append entry
        let mut wal = Wal::new(path).unwrap();
        let entry = create_test_entry(1, 1, b"test entry");
        wal.append(entry).unwrap();

        // Verify file exists and has content
        let metadata = fs::metadata(path).unwrap();
        assert!(metadata.len() > 0);
    }

    #[test]
    fn test_log_entry_encoding_format() {
        let entry = create_test_entry(0x1234567890ABCDEF, 0xFEDCBA0987654321, b"test");
        let encoded = entry.encode().unwrap();

        // Verify the encoding format
        assert_eq!(&encoded[0..8], &0x1234567890ABCDEFu64.to_le_bytes());
        assert_eq!(&encoded[8..16], &0xFEDCBA0987654321u64.to_le_bytes());
        assert_eq!(&encoded[16..24], &4u64.to_le_bytes()); // length of "test"
        assert_eq!(&encoded[24..28], b"test");
    }

    #[test]
    fn test_wal_multiple_decode_cycles() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_str().unwrap();

        let mut wal = Wal::new(path).unwrap();

        // Append entries
        for i in 1..=5 {
            let entry = create_test_entry(i, i, format!("cycle test {}", i).as_bytes());
            wal.append(entry).unwrap();
        }

        // Multiple replay cycles should be consistent
        for _ in 0..3 {
            let entries = wal.replay().unwrap();
            assert_eq!(entries.len(), 5);

            for (i, entry) in entries.iter().enumerate() {
                let expected_index = (i + 1) as u64;
                assert_eq!(entry.index, expected_index);
                assert_eq!(entry.term, expected_index);
                assert_eq!(entry.command, Bytes::from(format!("cycle test {}", expected_index)));
            }
        }
    }
}

