// HAS060アセンブラ（work.sのワークエリア）との完全な互換性を確保するためのアセンブリ状態コンテキスト
// フィールドをすべて網羅・定義しており、現段階のアセンブラパス実装でまだ使われていない状態フラグ等の警告を抑制するために付与しています。
#![allow(dead_code)]
use super::cpu::CpuType;
use super::scd::ScdTemp;
use super::section::{Section, N_SECTIONS};
use crate::options::Options;

/// アセンブルパス（has.equ / work.s: ASMPASS）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsmPass {
    Pass1 = 1,
    Pass2 = 2,
    Pass3 = 3,
}

/// アセンブルコンテキスト
///
/// アセンブル処理全体を通じて状態を保持する構造体。
/// `work.s` のワークエリアに対応する。
///
/// Phase 1 は Options を保持するだけの最小骨格。
/// 後のフェーズでシンボルテーブル・中間コードバッファ等が追加される。
#[derive(Debug)]
pub struct AssemblyContext {
    // ---- オプション ----
    pub opts: Options,

    // ---- アセンブルパス ----
    pub pass: AsmPass,

    // ---- セクション / ロケーションカウンタ ----
    /// 現在のセクション番号
    pub section: Section,
    /// 各セクションのロケーションカウンタ（LOCCTRBUF）
    pub loc_ctr: [u32; N_SECTIONS],
    /// 各セクションの最適化ロケーションオフセット（LOCOFFSETBUF）
    pub loc_offset: [u32; N_SECTIONS],
    /// 行頭のロケーションカウンタ（LTOPLOC）
    pub loc_top: u32,

    // ---- CPU ----
    /// 現在の CPU 情報（CPUNUMBER, CPUTYPE）
    pub cpu: CpuType,
    /// FPU コプロセッサ ID（.fpid で設定、0..7）
    pub fpid: u8,

    // ---- エラー ----
    /// エラー数（NUMOFERR）
    pub num_errors: u32,
    /// ワーニング数
    pub num_warnings: u32,

    // ---- 状態フラグ ----
    /// .end 命令が現れた（ISASMEND）
    pub is_asm_end: bool,
    /// オブジェクト出力を抑制する（ISOBJOUT）
    pub is_obj_out_suppressed: bool,
    /// .align 使用時の最大アライン値（MAXALIGN, 2^n の n）
    pub max_align: u8,
    /// 相対セクション情報を出力する（MAKERSECT）
    pub make_rel_sect: bool,

    // ---- .offset モード（SECT_ABS, オリジナルの SECTION=0 に対応）----
    /// .offset モード中か（仮想オフセットセクション）
    pub is_offset_mode: bool,
    /// .offset セクションのロケーションカウンタ
    pub offset_loc: u32,
    /// `.offsym <expr>,<sym>` でシンボル指定付きオフセットモード中か
    pub offsym_with_symbol: bool,
    /// `.ln` で指定されたSCD行番号
    pub scd_ln: u16,
    /// SCDデバッグ用のソースファイル名（`.file`）
    pub scd_file: Vec<u8>,
    /// SCD疑似命令が有効化されたか（`.file` 検出後に true）
    pub scd_enabled: bool,
    /// SCD 拡張シンボル一時バッファ（`.def`〜`.endef`）
    pub scd_temp: ScdTemp,

    // ---- 条件アセンブル ----
    /// .if のネスト深度（IFNEST）
    pub if_nest: u16,
    /// .if スキップ中のネスト深度（IFSKIPNEST）
    pub if_skip_nest: u16,
    /// .if の不成立部スキップ中（ISIFSKIP）
    pub is_if_skip: bool,

    // ---- requestファイル ----
    /// `.request` で収集したファイル名（$E001 レコード出力用）
    pub request_files: Vec<Vec<u8>>,
    // ---- PRN制御 ----
    /// PRNリストへの行出力可否（.list/.nlist で制御）
    pub prn_listing: bool,
    /// PRNリストでマクロ展開行を出力するか（.lall/.sall で制御）
    pub prn_macro_listing: bool,
    /// `.title` で指定されたPRNタイトル文字列
    pub prn_title: Vec<u8>,
    /// `.subttl` で指定されたPRNサブタイトル文字列
    pub prn_subttl: Vec<u8>,
}

impl AssemblyContext {
    /// オプションからコンテキストを初期化する
    pub fn new(opts: Options) -> Self {
        let cpu = opts.cpu;

        AssemblyContext {
            opts,
            pass: AsmPass::Pass1,

            section: Section::Text,
            loc_ctr: [0u32; N_SECTIONS],
            loc_offset: [0u32; N_SECTIONS],
            loc_top: 0,

            cpu,
            fpid: 1,

            num_errors: 0,
            num_warnings: 0,

            is_asm_end: false,
            is_obj_out_suppressed: false,
            max_align: 0,
            make_rel_sect: false,

            is_offset_mode: false,
            offset_loc: 0,
            offsym_with_symbol: false,
            scd_ln: 0,
            scd_file: Vec::new(),
            scd_enabled: false,
            scd_temp: ScdTemp::default(),

            if_nest: 0,
            if_skip_nest: 0,
            is_if_skip: false,

            request_files: Vec::new(),
            prn_listing: true,
            prn_macro_listing: false,
            prn_title: Vec::new(),
            prn_subttl: Vec::new(),
        }
    }

    // ---- ロケーションカウンタ操作 ----

    /// 現在のセクションのロケーションカウンタを返す
    pub fn location(&self) -> u32 {
        if self.is_offset_mode {
            self.offset_loc
        } else {
            self.loc_ctr[self.section as usize - 1]
        }
    }

    /// 現在のセクションのロケーションカウンタを進める
    pub fn advance_location(&mut self, bytes: u32) {
        if self.is_offset_mode {
            self.offset_loc = self.offset_loc.wrapping_add(bytes);
        } else {
            let idx = self.section as usize - 1;
            self.loc_ctr[idx] = self.loc_ctr[idx].wrapping_add(bytes);
        }
    }

    /// セクションを切り替える（.offset モードを解除する）
    pub fn set_section(&mut self, sec: Section) {
        self.is_offset_mode = false;
        self.offsym_with_symbol = false;
        self.section = sec;
    }

    /// .offset モードに切り替える（SECT_ABS）
    pub fn set_offset_mode(&mut self, v: u32) {
        self.is_offset_mode = true;
        self.offset_loc = v;
    }

    // ---- CPU 操作 ----

    /// CPU を変更する（.cpu 疑似命令用）
    pub fn set_cpu(&mut self, cpu: CpuType) {
        self.cpu = cpu;
    }

    /// 現在のCPU情報を CpuType として取得
    pub fn get_cpu_type(&self) -> CpuType {
        self.cpu
    }

    // ---- エラー処理 ----

    /// エラー数をインクリメントして返す
    pub fn add_error(&mut self) -> u32 {
        self.num_errors += 1;
        self.num_errors
    }

    /// ワーニング数をインクリメント
    pub fn add_warning(&mut self) {
        self.num_warnings += 1;
    }

    /// アセンブルが成功したか（エラーなし）
    pub fn is_success(&self) -> bool {
        self.num_errors == 0
    }

    // ---- ワーニングレベル ----

    /// 実効ワーニングレベルを返す
    pub fn effective_warn_level(&self) -> u8 {
        self.opts.effective_warn_level()
    }
}
