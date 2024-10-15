use std::{env, fs, path::PathBuf};
use avif_parse::AvifData;

fn main() {
    env_logger::init();

    let path = PathBuf::from(env::args_os().nth(1).expect("Specify path to an AVIF file"));
    let file = fs::read(&path).unwrap();
    let avif = AvifData::from_reader(&mut file.as_slice()).unwrap();

    println!("{:#?}", avif.primary_item_metadata().unwrap());

    // You can view these OBUs at https://mdakram.com/media-parser-gui/#/av1

    let av1_out = path.with_extension("av1");
    fs::write(&av1_out, &avif.primary_item).unwrap();
    println!("Written {}", av1_out.display());

    if let Some(alpha_data) = avif.alpha_item.as_deref() {
        let av1_out = path.with_extension(if avif.premultiplied_alpha { "prem.av1"} else {"alpha.av1"});
        fs::write(&av1_out, alpha_data).unwrap();
        println!("Written {}", av1_out.display());
    }
}
