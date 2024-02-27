use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    process::Stdio,
};
use temp_dir::TempDir;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

pub async fn pdf_to_text(pdf_file: &PathBuf) -> Result<String, String> {
    let pdf_file_bytes = fs::read(pdf_file).unwrap();

    let mut child = Command::new("pdftotext")
        .args(["-layout", "-", "-"])
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .as_mut()
        .ok_or("Child process stdin has not been captured!")
        .unwrap()
        .write_all(&pdf_file_bytes)
        .await
        .unwrap();

    let output = child.wait_with_output().await.unwrap();
    if !output.status.success() {
        let err = String::from_utf8(output.stderr).unwrap();
        return Err(format!("External command failed:\n {}", err));
    }

    let raw_output = String::from_utf8(output.stdout).unwrap();
    let mut page_output: Vec<String> = raw_output.split('\u{C}').map(|x| x.to_string()).collect();

    let binding = TempDir::with_prefix("hexane").unwrap();
    let temp_dir = binding.path();

    let mut child = Command::new("pdfimages")
        .args(["-p", "-", ""])
        .stdin(Stdio::piped())
        .current_dir(temp_dir.to_str().unwrap())
        .spawn()
        .unwrap();

    child
        .stdin
        .as_mut()
        .ok_or("Child process stdin has not been captured!")
        .unwrap()
        .write_all(&pdf_file_bytes)
        .await
        .unwrap();

    if !child.wait().await.unwrap().success() {
        return Err("External command failed".into());
    }

    let mut file_hash_seen: HashSet<String> = HashSet::new();

    for entry in fs::read_dir(temp_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let metadata = fs::metadata(&path).unwrap();

        if !metadata.is_file() {
            continue;
        }

        Command::new("convert")
            .args([
                "-units",
                "PixelsPerInch",
                path.to_str().unwrap(),
                "-resample",
                "300",
                path.to_str().unwrap(),
            ])
            .output()
            .await
            .unwrap();

        let image_data = fs::read(&path).unwrap();
        let file_hash = Sha256::digest(<Vec<u8> as AsRef<[u8]>>::as_ref(&image_data))
            .iter()
            .map(|b| format!("{:02x}", b).to_string())
            .collect::<String>();

        if file_hash_seen.contains(&file_hash) {
            continue;
        }
        file_hash_seen.insert(file_hash);

        // tracing::trace!("running tesseract on file: {} ({})", &pdf_file.display(), &entry.path().display());
        let mut child = Command::new("tesseract")
            .args(["--dpi", "300", "-", "-", "tsv"])
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();

        child
            .stdin
            .as_mut()
            .ok_or("Child process stdin has not been captured!")
            .unwrap()
            .write_all(&image_data)
            .await
            .unwrap();

        let output = child.wait_with_output().await.unwrap();
        if output.status.success() {
            let raw_output = String::from_utf8(output.stdout).unwrap();
            let output_page_number: usize = path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .split('-')
                .nth(1)
                .unwrap()
                .parse::<usize>()
                .unwrap();

            let parsed_output = tesseract_tsv_to_text(&raw_output);
            page_output[output_page_number].push_str(&parsed_output);
        } else {
            // let err = String::from_utf8(output.stderr).unwrap();
            // return Err(format!("External command failed:\n {}", err));
        }
    }

    Ok(page_output.join("\n"))
}

#[derive(Debug)]
struct OcrRow<'a> {
    top: u32,
    left: u32,
    text: &'a str,
    width: u32,
}

fn tesseract_tsv_to_text(file: &str) -> String {
    let mut output: String = String::new();

    let binding = file
        .split('\n')
        .map(|line| line.split('\t').collect::<Vec<&str>>())
        .collect::<Vec<Vec<&str>>>();

    let (headers, tsv_rows) = binding.split_first().unwrap();

    let ocr_rows: Vec<OcrRow> = tsv_rows
        .iter()
        .filter_map(|tsv| {
            let mut ocr_row: HashMap<&str, &str> = HashMap::new();
            for i in 0..tsv.len() {
                let key = headers[i];
                match key {
                    "level" | "top" | "left" | "text" | "width" => {
                        ocr_row.insert(key, tsv[i]);
                    }
                    _ => continue,
                }
            }

            if *ocr_row.get("level").unwrap() != "5"
                || ocr_row.get("text").unwrap().trim().is_empty()
            {
                return None;
            }

            Some(OcrRow {
                top: ocr_row.get("top").unwrap().parse().unwrap(),
                left: ocr_row.get("left").unwrap().parse().unwrap(),
                width: ocr_row.get("width").unwrap().parse().unwrap(),
                text: ocr_row.get("text").unwrap(),
            })
        })
        .collect();

    let mut counter: u32 = u32::MAX;
    for i in 0..ocr_rows.len() - 1 {
        if ocr_rows[i + 1].left > (ocr_rows[i].left + ocr_rows[i].width) {
            counter = std::cmp::min(
                counter,
                ocr_rows[i + 1].left - (ocr_rows[i].left + ocr_rows[i].text.len() as u32),
            );
        }
    }
    let pixel_to_space = counter - 1;

    let mut pixel_top = 0;
    let mut pixel_left = 0;
    let mut pixel_left_actual = 0;

    for row in ocr_rows {
        if pixel_left > row.left && row.top > pixel_top {
            output.push_str("\n");
            pixel_left = 0;
            pixel_left_actual = 0;
        }
        pixel_top = row.top;

        let mut spaces = std::cmp::max(1, (row.left - pixel_left) / pixel_to_space);
        let spaces_actual = std::cmp::max(1, (row.left - pixel_left_actual) / pixel_to_space);

        if spaces > 2 && spaces_actual > 4 && pixel_left_actual < pixel_left {
            spaces = spaces_actual;
        }

        output.push_str(
            &std::iter::repeat(' ')
                .take(spaces as usize)
                .collect::<String>(),
        );
        output.push_str(row.text);

        pixel_left = row.left + row.width;
        pixel_left_actual +=
            (spaces * pixel_to_space) + (row.text.chars().count() as u32 * pixel_to_space);
    }

    output
}
