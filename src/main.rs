use exitcode;

use std::env;
use std::fs;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::path::PathBuf;
use std::process;

use bitreader::BitReader;
use structopt::StructOpt;

/*
Note from https://stackoverflow.com/a/4678183

ADTS sample rates and channel counts are for HE-AAC and HE-AACv2 to try to maintain
compatibility with LC only decoders. The good news is that it they are inaccurate in
a precise manner. HE-AAC will report half the sample rate and HE-AACv2 will always
report a mono stream. This is because HE-AAC adds SBR which double the sample rate
and HE-AACv2 adds parametric stereo to SBR and PS turning a mono stream into a stereo
image. The SBR payload lives inside an AAC fill element which is ignored by an LC only
encoder and the PS payload lives inside the SBR payload.

Some decoders assume SBR if the sample rate <= 24kHz and always decode mono streams to
stereo to avoid detecting these features up front. In that case the SBR decoder can be
run in a pure upsampling mode if SBR data is not found.
*/

#[derive(Debug, StructOpt)]
struct CliArgs {
    /// Input File
    filepath: PathBuf,
    /// Offset within the input file
    #[structopt(default_value = "0")]
    offset: u32,
}

const ADTS_HDR_MIN_LEN: usize = 7;
const ADTS_HDR_MAX_LEN: usize = 9;

fn find_startcode(buf: [u8; ADTS_HDR_MAX_LEN]) -> Option<usize> {
    buf.windows(2)
        .position(|b| (b[0] == 0xFF) && ((b[1] & 0xF0) == 0xF0))
}

fn seek_startcode(mut file: &fs::File) -> std::io::Result<u64> {
    let mut buffer = [0; ADTS_HDR_MAX_LEN as usize];

    loop {
        file.read_exact(&mut buffer)?;
        let startcode_pos = match find_startcode(buffer) {
            Some(n) => n,
            None => {
                //println!("Not found anything at {0}", )
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

#[derive(Debug)]
enum MPEGAudioObjectType {
    NULL = 0,
    AAC_MAIN = 1,
    AAC_LC = 2,
    AAC_SSR = 3,
    AAC_LTP = 4,
    SBR = 5,
    AAC_SCALABLE = 6,
    TWIN_VQ = 7,
    CELP = 8,
    LAYER1 = 32,
    LAYER2 = 33,
    LAYER3 = 34,
}

struct ADTSHeader {
    syncword: u16,
    id: MPEGVersion,
    protection_absent: bool,
    profile: MPEGAudioObjectType,
    sampling_frequency_index: u8,
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
        _ => return None,
    };

    // Layer (always 0)
    reader.skip(2).ok()?;

    // Protection absend
    let protection_absent = reader.read_bool().ok()?;

    // Profile
    //reader.skip(2).ok()?;
    let profile = match reader.read_u8(2).ok()? {
        0 => MPEGAudioObjectType::AAC_MAIN,
        1 => MPEGAudioObjectType::AAC_LC,
        2 => MPEGAudioObjectType::AAC_SSR,
        3 => MPEGAudioObjectType::AAC_LTP,
        4 => MPEGAudioObjectType::SBR,
        5 => MPEGAudioObjectType::AAC_SCALABLE,
        6 => MPEGAudioObjectType::TWIN_VQ,
        7 => MPEGAudioObjectType::CELP,
        31 => MPEGAudioObjectType::LAYER1,
        32 => MPEGAudioObjectType::LAYER2,
        33 => MPEGAudioObjectType::LAYER3,
        _ => return None,
    };

    // Sampling frequency index
    //reader.skip(4).ok()?;
    let sampling_frequency_index = reader.read_u8(4).ok()?;

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

    file.seek(SeekFrom::Current(-(ADTS_HDR_MIN_LEN as i64)))
        .ok()?;

    return Some(ADTSHeader {
        syncword,
        id: mpeg_version,
        profile: profile,
        sampling_frequency_index: sampling_frequency_index,
        protection_absent,
        frame_length,
    });
}

fn main() {
    // Argument handling
    let opts = CliArgs::from_args();

    println!(
        "Reading file '{0}' starting at {1}",
        opts.filepath.display(),
        opts.offset
    );

    let mut file = match fs::OpenOptions::new().read(true).open(opts.filepath) {
        Ok(result) => result,
        Err(err) => {
            eprintln!("error: failed opening file: {0}", err);
            process::exit(exitcode::NOINPUT);
        }
    };

    match file.seek(SeekFrom::Current(opts.offset as i64)) {
        Ok(_) => {}
        Err(err) => {
            eprintln!("error: failed seeking to offset: {}", opts.offset);
            process::exit(exitcode::DATAERR);
        }
    }

    // Read header
    match seek_startcode(&file) {
        Ok(pos) => {
            println!("Found startcode at offset {}", pos);
        }
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

        let cur_pos = file
            .seek(SeekFrom::Current(0))
            .expect("failed obtaining current file position");
        println!("Header at: {}", cur_pos);
        println!("Len is {}", header.frame_length);
        println!("ID is {:?}", header.id);
        println!("Profile is {:?}", header.profile);
        println!(
            "Sampling frequency index is {:?}",
            header.sampling_frequency_index
        );
        match file.seek(SeekFrom::Current(header.frame_length as i64)) {
            Ok(_) => {}
            Err(err) => {
                eprintln!("error: Failed seeking to next header: {}", err);
                process::exit(exitcode::DATAERR);
            }
        };
    }
}
