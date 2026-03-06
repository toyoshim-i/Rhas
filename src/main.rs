// rhas - X68000 HAS060X assembler ported to Rust
// Based on HAS060X.X by TcbnErik, HAS060.X by M.Kamada, HAS.X v3.09 by Y.Nakamura

mod addressing;
mod context;
mod instructions;
mod error;
mod expr;
mod object;
mod options;
mod pass;
mod source;
mod symbol;

use options::{parse_args, ParseError};
use std::io::Write;
use std::path::PathBuf;

fn main() {
    // 実行ファイル名から g2as モードかどうかを判定（main.s: docmdline 冒頭）
    let g2as_mode = std::env::args()
        .next()
        .map(|p| {
            PathBuf::from(&p)
                .file_name()
                .map(|n| n.to_string_lossy().to_ascii_lowercase().starts_with("g2as"))
                .unwrap_or(false)
        })
        .unwrap_or(false);

    // argv[1..] をコマンドライン引数として渡す（argv[0] は除く）
    let args: Vec<_> = std::env::args().skip(1).collect();

    let stderr = std::io::stderr();
    let mut err_out = stderr.lock();

    let opts = match parse_args(args.iter().map(|s| s.as_str()), g2as_mode) {
        Ok(o) => o,
        Err(ParseError::Usage(msg)) => {
            // Usage エラー: タイトル + 使用法を表示して終了
            print!("{}", options::usage_message());
            if !msg.is_empty() {
                let _ = writeln!(err_out, "エラー: {}", msg);
            }
            std::process::exit(1);
        }
        Err(ParseError::MultipleSourceFiles) => {
            let _ = writeln!(err_out, "エラー: 複数のファイル名は指定できません");
            std::process::exit(1);
        }
    };

    // タイトル表示（-l オプション）
    if opts.disp_title {
        print!("{}", options::title_message());
    }

    // ソースファイルが指定されているか確認し、バイト列を取得
    let source_file_bytes = if let Some(sf) = &opts.source_file {
        sf
    } else {
        print!("{}", options::usage_message());
        std::process::exit(1);
    };

    // 出力ファイル名を決定
    let output_path: PathBuf = if let Some(ref o) = opts.object_file {
        PathBuf::from(String::from_utf8_lossy(o).as_ref())
    } else {
        // ソースファイルの拡張子を .o に変換
        let src = PathBuf::from(String::from_utf8_lossy(source_file_bytes).as_ref());
        src.with_extension("o")
    };

    // コンテキストを作成
    let mut ctx = context::AssemblyContext::new(opts);

    // アセンブル実行
    // オリジナルと同様、エラー/成功メッセージは標準出力へ（main.s 参照）
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    match pass::assemble(&mut ctx) {
        Ok(result) => {
            // オブジェクトファイルを書き出し
            match std::fs::write(&output_path, &result.obj_bytes) {
                Ok(_) => {
                    // 成功: "エラーはありません"（main.s: no_msg）
                    let _ = writeln!(out, "エラーはありません");
                    std::process::exit(0);
                }
                Err(e) => {
                    let _ = writeln!(err_out, "エラー: 出力ファイルを書き出せません: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Err(pass::AssembleError::SourceNotFound(path)) => {
            let _ = writeln!(err_out, "エラー: ソースファイルが見つかりません: {}",
                path.display());
            std::process::exit(1);
        }
        Err(pass::AssembleError::HasErrors(n)) => {
            // オリジナル: "エラーが N 個ありました．アセンブルを中止します"（main.s: fatal_msg1/2）
            let _ = writeln!(out, "エラーが {} 個ありました．アセンブルを中止します", n);
            std::process::exit(1);
        }
        Err(pass::AssembleError::Io(e)) => {
            let _ = writeln!(err_out, "IOエラー: {}", e);
            std::process::exit(1);
        }
    }
}
