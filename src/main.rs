use exitcode;

use std::process;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;

use bitreader::BitReader;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
struct CliArgs {
    /// Input File
    filepath: PathBuf,
    /// Offset within the input file
    #[structopt(default_value = "0")]
    offset: u32,
}

fn find_startcode(buffer : [u8; 9]) -> Option<usize> {

    // Only iterate to len -1 so we can always lookahead
    for x in 0..(buffer.len() - 1) {
        if buffer[x] == 0xFF && (buffer[x+1] & 0xF0) == 0xF0 {
            return Some(x);
        }
    }
    None
}

const ADTS_HDR_MIN_LEN: i8 = 7;
const ADTS_HDR_MAX_LEN: i8 = 9;

fn seek_startcode(mut file: &fs::File) -> std::io::Result<u64> {
    let mut buffer = [0; ADTS_HDR_MAX_LEN as usize];

    loop {
        file.read_exact(&mut buffer)?;
        let startcode_pos = match find_startcode(buffer) {
            Some(n) => n,
            None => {
                file.seek(SeekFrom::Current(ADTS_HDR_MAX_LEN as i64 - 4))?;
                continue;
            }
        };

        // Seek back to header start
        let offset = -(ADTS_HDR_MAX_LEN as i64 - startcode_pos as i64);
        return file.seek(SeekFrom::Current(offset));
    }

    // Unreachable
}

#[derive(Debug)]
enum MPEGVersion {
    MPEG4 = 0,
    MPEG2 = 1,
}

struct ADTSHeader {
    syncword: u16,
    id: MPEGVersion,
    protection_absent: bool,
    //profile: u8,
    //sampling_frequency_index: u8,
    //channel_configuration: u8,
    frame_length: u16,
    //adts_buffer_fullness: u16,
    //num_raw_data_blocks: u8,
    //crc: u16,
}

fn peek_header(mut file: &fs::File) -> Option<ADTSHeader> {
    //let mut header : ADTSHeader;   
    let mut buffer = [0; ADTS_HDR_MIN_LEN as usize];

    file.read_exact(&mut buffer).ok()?;
    let mut reader = BitReader::new(&buffer);

    // Check syncword
    let syncword = reader.read_u16(12).ok()?;
    if syncword != 0xFFF {
        return None;
    }

    // MPEG Version
    let mpeg_version = match reader.read_u8(1).ok()? {
        0 => MPEGVersion::MPEG4,
        1 => MPEGVersion::MPEG2,
        _ => return None
    };

    // Layer (always 0)
    reader.skip(2).ok()?;

    // Protection absend
    let protection_absent = reader.read_bool().ok()?;

    // Profile
    reader.skip(2).ok()?;
    // Sampling frequency index
    reader.skip(4).ok()?;
    // Private bit
    reader.skip(1).ok()?;
    // Channel config
    reader.skip(3).ok()?;
    // Originality
    reader.skip(1).ok()?;
    // Home
    reader.skip(1).ok()?;
    // Copyrighted ID
    reader.skip(1).ok()?;
    // Copyright ID start signal bit
    reader.skip(1).ok()?;

    // Frame length
    let frame_length = reader.read_u16(13).ok()?;

    // Buffer fullness
    reader.skip(11).ok()?;
    // Number of frames
    reader.skip(2).ok()?;

    // CRC (if protection absent is 0)
    // TODO

    file.seek(SeekFrom::Current(-ADTS_HDR_MIN_LEN as i64)).ok()?;

    return Some(ADTSHeader {
        syncword,
        id: mpeg_version,
        protection_absent,
        frame_length
    });
}

fn main() {
    // Argument handling
    let opts = CliArgs::from_args();

    println!("Reading file '{0}' starting at {1}", opts.filepath.display(), opts.offset);

    let mut file = match fs::OpenOptions::new().read(true).open(opts.filepath) {
        Ok(result) => result,
        Err(err) => {
            eprintln!("error: failed opening file: {0}", err);
            process::exit(exitcode::NOINPUT);
        }
    };

    // Read header
    match seek_startcode(&file) {
        Ok(pos) => {
            println!("Found startcode at offset {}", pos);
        },
        Err(err) => {
            eprintln!("error: failed seeking to startcode: '{}'", err);
            process::exit(exitcode::DATAERR);
        }
    };

    loop {
        let header = match peek_header(&file) {
            Some(val) => val,
            None => {
                eprintln!("error: Failed reading ADTS header");
                process::exit(exitcode::DATAERR);
            }
        };

        println!("Len is {}", header.frame_length);
        match file.seek(SeekFrom::Current(header.frame_length as i64)) {
            Ok(_) => {},
            Err(err) => {
                eprintln!("error: Failed seeking to next header: {}", err);
                process::exit(exitcode::DATAERR);
            }
        };
    }
}
