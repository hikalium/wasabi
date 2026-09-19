# Plan: IME の単文節漢字変換

目標: WasabiOS の日本語入力（`src/ime.rs`）に、利用者が区切った未確定文節を
対象とする漢字変換を加え、「今日の天気は雨です」を入力できるようにする。
段階は4つ、複数の概念を含む段階はさらに小さなコミットに分ける。各コミットは
1概念で、`book2/ch2:` 接頭辞を付けて ajimi でこの順に埋め込める粒度にする。

この文書中の Rust コードは実装方針を示すスケッチであり、そのまま貼れる
パッチではない。

## 現状

- `src/ime.rs` はローマ字→かな変換までできている。スペースキーは
  「`pending_string` 全体が `きょう` に一致したら `今日`、それ以外は全角スペース」
  というハードコード。
- 確定済み文字列と未確定文字列の区別がない。`Console`（`src/cui.rs`）の
  `input_buf` は IME の `pending_string` の写しで、`render_ime_line` は
  両者の文字列差分を Backspace と再描画で画面に反映している。
- `set_pending` は次の3経路で呼ばれる: Ctrl+Space による IME 有効化時、
  CursorUp による履歴呼び出し後、Enter によるコマンド実行後。
- Enter は `Console` が直接処理し、IME には渡さず、常にコマンドを実行する。
- IME の有効状態は `is_ime_enabled`/`set_ime_enabled` のグローバルで、
  全 Console と `ime` コマンドが共有する。
- `print!` の出力はシリアル、VRAM の `BitmapTextWriter`（`src/graphics.rs`）、
  TCP ミラー、LPSS UART の4系統へ同じ文字列として流れる。VRAM 側は文字ごとに
  8×16 または 16×16 のグリフを `draw_font_fg` で描く。`draw_font_fg` は
  前景画素だけを描くので、同じ文字を重ね描きしても余分な画素は消えない。
- VRAM の `\x08` は 8 ピクセル戻って 8×16 を黒で塗る。16 幅グリフは
  `Console` が `\x08` を2回出して消している。シリアル等の端末では `\x08` は
  カーソル移動のみで消去はしない（現状の `render_ime_line` は VRAM の破壊的
  Backspace に依存している）。
- `get_glyph_width` は未収録文字に 0 を返すが、`Console` の幅計算は
  「16 なら 2 セル、それ以外は 1 セル」なので、未収録文字で位置がずれる。
- 漢字グリフは `third_party/unifont/chars_to_include.txt` に列挙して
  `make -C third_party/unifont` で `glyphs.txt` を再生成する。現在の漢字は
  `日本語今` の4文字。**全角スペース（U+3000）と、ローマ字表が生成する
  `ぃ`/`ぇ` は未収録**なので、IME が全角スペースを出しても何も描かれない。
- `glyphs.txt` の生成規則は `grep '$$' glyph_*.txt` で、一覧に基づいて
  生成された個別ファイルではなく、作業ディレクトリにある `glyph_*.txt` を
  すべて連結している。一覧から削った文字が残る。

## 表示の保証範囲

本計画では、**入力行が画面右端で折り返さず、編集中に別の出力（ログや他の
Console）が割り込まない**ことを前提にする。折り返しをまたぐ Backspace や
非同期出力後の入力行復元は、コンソール描画の別課題として本計画に含めない。

## 目標の入力手順

文節ごとにスペースで変換し、Enter で確定する。未確定の文節には下線が付き、
Enter で下線が消えて確定済みになる。未確定が空のときの Enter は従来どおり
コマンド実行になる。

```
kyouno [Space] [Enter]   → 今日の
tenkiha [Space] [Enter]  → 天気は
amedesu [Space] [Enter]  → 雨です
```

第4段階まで進むと、途中の Enter を省略して
`kyouno [Space] tenkiha [Space] amedesu [Space] [Enter]` でも入力できる。

変換は「未確定文字列の先頭にある辞書語を置き換え、残りはそのまま残す」方式。
文節は「自立語＋付属語」なので目標の文はこれで処理できるが、残りが助詞・
助動詞かどうかの判定はしない（例: `あめりか` は `雨りか` になる）。本文では
「利用者が文節を区切る、辞書の最長接頭辞置換による単文節変換」と説明する。

## 実装段階

### 1. 未確定文字列の下線表示と Enter による確定

3コミットに分ける。1a は見た目を変えない内部モデルの分離、1b は描画側の
下線対応、1c で両者をつなぐ。

#### 1a. 確定済み／未確定の分離と Enter 確定

変更ファイル: `src/ime.rs`, `src/cui.rs`

状態の所有権を次のように決める。

- 確定済み文字列は `Console::input_buf` が所有する。
- 未確定文字列は `InputMethodEditor::pending_string` が所有する。Console は
  別コピーを持たず、`ime.pending()` で参照する。

IME に追加するもの:

```rust
impl InputMethodEditor {
    pub fn pending(&self) -> &str;
    /// 未確定文字列を取り出して空にする（確定用）。
    pub fn take_pending(&mut self) -> String;
    pub fn clear(&mut self);
}
```

`set_pending` の3か所の呼び出しは次のように置き換え、`set_pending` 自体は
削除する。

| 元の呼び出し | 置き換え |
|---|---|
| Ctrl+Space での有効化時 | 確定済みを IME にコピーしない（何もしない） |
| CursorUp の履歴呼び出し後 | 未確定があれば先に確定し、履歴と `input_buf` を交換、IME は `clear()` |
| Enter でのコマンド実行後 | `clear()`（未確定は空のはずなので実質 no-op） |

Console のキー処理:

- **Enter**: `ime.pending()` が空でなければ `take_pending()` を `input_buf` に
  追記して再描画するだけ。空のときだけコマンドを実行する。
- **Backspace**: IME が `UpdatePendingString` を返したら未確定側だけが変わった
  ということなので `input_buf` は触らない。未確定が空のときは IME が
  `PassThrough` を返すので `input_plain` で確定済み末尾を消す。

  | 操作前 | 結果 |
  |---|---|
  | 未確定が 2 文字以上 | 未確定の末尾を 1 文字削除 |
  | 未確定が 1 文字 | 未確定を空にし、イベントを消費（確定済みは削らない） |
  | 未確定が空、確定済みあり | `PassThrough` により確定済み末尾を削除 |
  | 両方空 | 変更なし |

- **CursorUp**: `prev_cmd` が `None` なら何もしない（未確定も維持）。
  あれば未確定を確定してから交換する。
- **Ctrl+Space（IME オフ）**: 未確定をそのまま確定する。オンにするときは
  確定済みを取り込まない。
- **外部からの IME オフ**（`ime off` コマンド）: `handle_key_down` の先頭で
  「IME が無効なのに `ime.pending()` が空でない」なら先に確定する。
  世代カウンタ等の仕組みは入れない。

変換ロジックは変えず、既存 `today_conversion` はそのまま通ること。

テスト（`#[test_case]`、`Console` と同じモジュール内でフィールドを直接検査）:

- `enter_commits_pending_before_running_command`: `a` → `input_buf == ""`,
  `pending == "あ"`; Enter → `input_buf == "あ"`, `pending == ""`;
  もう一度 Enter でコマンド実行経路に入る。
- `deleting_last_pending_char_keeps_committed_text`: `input_buf = "A"`,
  `a`, BS → `input_buf == "A"`, `pending == ""`; BS → `input_buf == ""`。
- `cursor_up_without_history_preserves_pending`
- `cursor_up_commits_pending_then_recalls`
- `disabling_ime_commits_pending`
- `enabling_ime_does_not_import_committed_text`

説明する概念: 確定と未確定を内部状態として区別し、Enter を「確定」と
「実行」の2段階にする。未確定文字列がそのまま変換対象の文節になるので、
読みの切り出しはこの段階で達成される。

#### 1b. `BitmapTextWriter` の下線 SGR 対応

変更ファイル: `src/graphics.rs`

ANSI の SGR シーケンス `\x1b[4m`（下線開始）と `\x1b[24m`（下線終了）を
解釈する。シリアルと TCP 側の端末はこのシーケンスをそのまま下線として
表示するので、4系統で同じ文字列を流すだけで見た目がそろう。

パーサー状態は `write_str` をまたいで保持する。`fmt::write` は `write_str`
を複数回に分けて呼ぶことがあり、`"\x1b"`, `"[4"`, `"m今日"` と分割されても
一括入力と同じ結果にならなければならない。ESC は `lookup_font_8x16` の対象に
なりうるので、通常のグリフ描画より前に取り除く。

```rust
enum EscapeState {
    Ground,
    Escape,            // ESC を受けた直後
    Csi { param: u32 } // ESC [ の後、数字と ; を蓄積中
}

pub struct BitmapTextWriter<'a, T> {
    buf: &'a Mutex<T>,
    cursor_x: i64,
    cursor_y: i64,
    underline: bool,
    esc: EscapeState,
}
```

`BitmapTextWriter::new` は static 初期化で使われているので `const fn` のまま。

状態遷移（最小限）:

| 状態 | 入力 | 処理 |
|---|---|---|
| Ground | ESC | Escape へ |
| Ground | その他 | 従来の処理（改行・Backspace・グリフ描画） |
| Escape | `[` | Csi へ |
| Escape | その他 | 未対応として捨て Ground へ |
| Csi | 数字 | `param` に蓄積 |
| Csi | `;` | `param` を適用して 0 に戻す（`0;4m` のような複数パラメータ） |
| Csi | `m` | `param` を適用して Ground へ |
| Csi | その他の終端文字 | 未対応 CSI として捨て Ground へ |
| Csi | 改行・Backspace | シーケンスを破棄し、その文字を従来どおり処理 |

SGR パラメータは `4` → 下線オン、`24` → 下線オフ、`0` → リセット
（下線オフ）、それ以外は無視。SGR はカーソルを進めない。

描画: グリフ描画後、`underline` が立っていれば `draw_font_fg` が返した幅
`dx` のぶん `y + 15` の行に前景色で線を引く。`dx == 0`（未収録）なら引かない。
Backspace の消去は従来どおり 8×16 の矩形塗りで、下線も一緒に消える。

テスト（既存の `graphics.rs` テストと同じ `BitmapBuffer` を使う）:

- `sgr_survives_split_write_str_calls`: `"\x1b"`, `"[4"`, `"mA"` の順に
  書いて `underline == true`, `cursor_x == 8`；`"\x1b[2"`, `"4m"` で
  `underline == false`, `cursor_x` 不変。
- `sgr_whole_and_split_input_produce_identical_pixels`
- `underline_spans_ascii_and_wide_glyphs`: `A` は 8 ピクセル、`日` は
  16 ピクセルの下線。
- `two_backspaces_erase_wide_glyph_and_underline`
- `unsupported_csi_does_not_draw_parameter_text`: `\x1b[2J` などで数字が
  描かれない。
- `sgr_does_not_advance_cursor`

説明する概念: 文字列のなかに「表示属性の切り替え」を埋め込む制御シーケンス
と、ストリームをまたぐ状態機械。

#### 1c. 属性を含む差分描画で Console をつなぐ

変更ファイル: `src/cui.rs`, `third_party/unifont/chars_to_include.txt`,
`third_party/unifont/glyphs.txt`

全角スペース（U+3000）と `ぃ`/`ぇ` をグリフに追加する（既存の表示欠落の
修正であり、この段階で幅計算を統一するため必要）。

差分描画は「文字」だけでなく「下線の有無」も比較する。`draw_font_fg` は
前景画素しか描かないので、未確定の `今日` を確定して下線なしで重ね描きしても
下線は消えない。文字が同じでも属性が変われば消して描き直す必要がある。

```rust
#[derive(Clone, Copy, PartialEq, Eq)]
struct StyledChar { ch: char, underline: bool }

/// 確定済み（下線なし）＋未確定（下線あり）を並べたもの。
fn styled_line(committed: &str, pending: &str) -> Vec<StyledChar>;

/// BitmapTextWriter と同じ規則でセル数を返す（0 幅は 0 セル）。
fn glyph_cells(c: char) -> usize { (get_glyph_width(c) / 8) as usize }

/// 旧行→新行の差分を 1 本の文字列にエンコードする。
fn encode_line_diff(old: &[StyledChar], new: &[StyledChar]) -> String;
```

`encode_line_diff` の出力手順（共通接頭辞は `StyledChar` 全体で比較）:

1. `\x1b[24m` で下線をオフにする。
2. 旧末尾のセル数だけ `\x08` を出す。
3. 同じセル数の ASCII 空白を書く（端末では `\x08` が消去しないため）。
4. 同じセル数だけ `\x08` を出す。
5. 新末尾を、下線の切り替えを挟みながら書く。
6. 最後に `\x1b[24m` を出す。

1本の `String` にして `print!` を1回だけ呼ぶことで、少なくとも
`GLOBAL_PRINTER` のロック内では消去・再描画・属性解除が一続きになる。
`input_plain` と `render_ime_line` が別々に幅計算している構造をなくし、
通常入力・削除もこの経路に統合する。`Console` は描画前に旧行
（`styled_line(&input_buf, ime.pending())`）を取り、キー処理後の新行との
差分を出す。

テスト:

- `committing_same_text_requires_redraw`: 未確定 `今日` → 確定 `今日` で
  出力が空でない。
- `cell_width_matches_bitmap_writer`: `A`=1, `日`=2, U+3000=2,
  未収録=0。
- `commit_redraw_matches_plain_text_pixels`: `BitmapBuffer` に未確定の
  `今日` を描いてから差分を適用した結果が、最初から下線なしで `今日` を
  描いた bitmap と一致する。
- `shrinking_pending_clears_old_tail`
- `unchanged_line_produces_no_output`

説明する概念: 入力の確定状態と表示属性を分けて持ち、属性の変更も再描画の
原因として扱う。

### 2. 辞書の導入（データ化）

変更ファイル: `src/ime.rs`, `src/cui.rs`（初期化）, `src/font.rs`（グリフ
テスト）, `third_party/unifont/chars_to_include.txt`, `glyphs.txt`, `Makefile`

```rust
type Candidates = &'static [&'static str];

pub struct InputMethodEditor {
    // ...
    dict: BTreeMap<&'static str, Candidates>,
}

impl InputMethodEditor {
    pub fn init_dict(&mut self) {
        self.dict.insert("きょう", &["今日"]);
        self.dict.insert("てんき", &["天気"]);
        self.dict.insert("あめ", &["雨"]);
    }
}
```

`きょう`→`今日` の if 文を辞書検索に置き換える。この段階では
「未確定文字列全体で完全一致」のまま。既存 `today_conversion` は期待値を
維持し、初期化に `init_dict()` を足すだけ。

スペースの仕様をここで整理する。

| 未確定の状態 | Space |
|---|---|
| 空 | 全角スペースを 1 文字入れる |
| 辞書に完全一致 | 先頭候補へ置換 |
| 一致なし | 内容を変えずイベントを消費（同じ内容の `UpdatePendingString` を返す） |

一致なしで全角スペースを足す旧仕様はここで廃止する。`PassThrough` を返すと
確定済み側に ASCII 空白が入ってしまうので返さない。辞書の空キー・空候補列は
禁止する。

グリフ: `天気雨` を追加。あわせて `Makefile` の `glyphs.txt` 規則を
「一覧から生成した `$(GLYPHS_TO_INCLUDE)` だけを連結する」形に直し、
`chars_to_include.txt` を依存に加える（`grep -H '^' $(GLYPHS_TO_INCLUDE)`。
`-H` は対象が1ファイルでもヘッダーを出すため）。これで一覧から削った文字や
作業ディレクトリに残った余分な `glyph_*.txt` が混入しなくなる。

テスト（以降の IME テストで使う補助: `new_test_ime()` は
`init_romaji_map` と `init_dict` を呼ぶ。`type_text(&mut ime, "tenki ")` は
1文字ずつ `send_key_down` して最後の結果を返す）:

- `weather_conversion`: `tenki ` → `天気`
- `unmatched_space_is_consumed_without_insertion`: `sora ` → `そら`
- `empty_pending_space_inserts_fullwidth_space`
- `conversion_glyphs_are_available`: `今日天気雨` の各文字が幅 16

説明する概念: 変換規則をコードからデータへ分離する。

### 3. 付属語を残す最長一致（単文節変換の完成）

変更ファイル: `src/ime.rs`, `src/cui.rs`（結合テスト）

```rust
struct DictionaryMatch<'a> {
    key: &'static str,
    candidates: Candidates,
    suffix: &'a str,
}

impl InputMethodEditor {
    fn longest_match<'a>(&self, reading: &'a str) -> Option<DictionaryMatch<'a>> {
        self.dict
            .iter()
            .filter(|(key, _)| reading.starts_with(**key))
            .max_by_key(|(key, _)| key.len())
            .map(|(&key, &candidates)| DictionaryMatch {
                key,
                candidates,
                suffix: &reading[key.len()..],
            })
    }
}
```

`starts_with` で確認済みの辞書キーの `len()` は文字境界なので、その位置で
切り出しても panic しない（任意のバイト位置で `&s[..n]` を作ってはいけない）。
候補＋接尾部を新しい `String` に組んでから `pending_string` を置き換える。

スペースの仕様は第2段階の表の「完全一致」を「最長接頭辞一致」に置き換える
だけ。接尾部はそのまま残し、ひらがな判定や品詞判定はしない。

テスト:

- `phrase_conversion_preserves_suffix`: `kyouno `→`今日の`,
  `tenkiha `→`天気は`, `amedesu `→`雨です`
- `longest_dictionary_prefix_wins`: テスト内で `きょ`→`短` を足しても
  `kyouno ` は `今日の`（`短` は文字列処理のテスト専用でグリフは要らない）
- `sentence_is_built_from_committed_phrases`（Console 結合）: 3 文節を
  Space, Enter で入れて `input_buf == "今日の天気は雨です"`
- `no_match_preserves_reading`
- `longest_prefix_is_not_grammatical_analysis`: `あめりか`→`雨りか` を
  現方式の動作範囲として固定する

説明する概念: 文節＝自立語＋付属語、利用者による文節区切りと最長接頭辞置換。

### 4. 複数候補の巡回と自動確定

2コミットに分ける。

#### 4a. 候補状態と巡回・取消

変更ファイル: `src/ime.rs`, `src/font.rs`, `chars_to_include.txt`, `glyphs.txt`

`あめ` の候補を `雨`/`飴` にし、`飴` のグリフを追加する。

```rust
struct Conversion {
    reading: String,        // 変換直前の未確定文字列（接尾部込み）
    suffix: String,
    candidates: Candidates,
    index: usize,
}

pub struct InputMethodEditor {
    pending_string: String,
    conversion: Option<Conversion>,
    // ...
}
```

状態の意味:

- `pending_string` 空、`conversion == None`: 未入力。
- `pending_string` 非空、`conversion == None`: 読みの編集中。
- `conversion == Some`: 候補選択中。`pending_string` は候補＋接尾部。

| 操作 | 読み編集中 | 候補選択中 |
|---|---|---|
| Space | 辞書検索して候補選択中へ | 次候補へ巡回（`(index + 1) % len`） |
| Backspace | 末尾 1 文字削除 | 元の読みへ戻すだけ（読みは削らない） |
| Enter / IME オフ / 履歴 | 現在の内容を確定 | 選択中候補を確定 |
| 通常文字 | ローマ字処理 | （このコミットでは）候補状態を解除して従来どおり末尾に追加 |

`take_pending`/`clear` は `conversion` も `None` に戻す。

テスト:

- `rain_candidates_cycle`: `ame `→`雨`, Space→`飴`, Space→`雨`
- `backspace_cancels_conversion_before_deleting_reading`:
  `amedesu ` の後 BS→`あめです`, BS→`あめで`
- `candidate_cycle_preserves_suffix`
- `commit_clears_conversion_state`
- `candy_glyph_is_available`

#### 4b. 変換後の文字入力で自動確定

変更ファイル: `src/ime.rs`, `src/cui.rs`

1 キーで「前の文節を確定」と「次の文節の先頭文字を入力」の両方を Console に
伝える必要があるので、結果型を拡張する。

```rust
pub enum InputEditResult {
    PassThrough,
    UpdatePendingString(String),
    /// 前文節を確定し、次文節の未確定を開始した。
    CommitAndUpdate { committed: String, pending: String },
}
```

`CommitAndUpdate` を返す時点で IME 内部の未確定は新文節の `pending` に
更新済み。Console は `committed` を `input_buf` に追記して再描画するだけ。

`send_key_down` の優先順位:

1. Backspace: 候補選択中なら取消、そうでなければ読みの削除。
2. Space: 候補選択中なら巡回、そうでなければ変換開始。
3. 通常文字: 候補選択中なら `take_pending()` で確定し、空の状態にその文字を
   ローマ字処理して `CommitAndUpdate`。そうでなければ従来どおり。
4. その他: `PassThrough`。

ESC・DEL などの制御文字は「別の文字」として自動確定の契機にせず、Console
側で無視する（ANSI 制御を入力行に混入させない）。

テスト:

- `typing_after_conversion_commits_and_starts_next_phrase`:
  `kyouno ` の後 `t` → `CommitAndUpdate { committed: "今日の", pending: "t" }`
- `sentence_can_be_entered_with_one_final_commit`（Console 結合）:
  `kyouno tenkiha amedesu ` を打って `input_buf == "今日の天気は"`,
  `pending == "雨です"`；Enter で `input_buf == "今日の天気は雨です"`
- `space_cycles_without_auto_commit`
- `backspace_cancels_without_auto_commit`
- `auto_commit_redraw_removes_old_underline`

説明する概念: 候補選択という状態を持ち、1 回の入力イベントから
「前文節の確定」と「次文節の開始」を同時に返す。

## Codex レビューで採用しなかった提案

- `Console` から `LineEditor` を切り出す案: テスト容易性は上がるが、書籍の
  コードとしては `Console` に閉じたほうが説明しやすい。テストは同一モジュール
  内でフィールドを直接検査する。
- IME 有効状態に世代カウンタを付ける案: `handle_key_down` の先頭で
  「無効なのに未確定が残っていれば確定」の 1 分岐で足りる。
- CSI パーサーの 32 バイトバッファと長さ超過処理: 自分が出す `\x1b[4m` と
  `\x1b[24m` を扱えれば十分なので、パラメータは `u32` 1 個の蓄積にとどめる。

## 補足

- グリフ追加は使う段階のコミットに同梱すると、そのコミット単体でビルドと
  表示が通る。`chars_to_include.txt` と生成済み `glyphs.txt` の両方を
  コミットする。
- 下線はグリフの最下行 `y + 15` に描くため、`g` や `y` のように最下行まで
  ピクセルがある字形では重なる。行の高さを 17 ピクセルにする案は改行・
  折り返し・消去領域に波及するので本計画には含めず、重なりを許容する。
- 入力手順の前提として、`kyou␣` の直後に `no` だけ打って `tenki` を続けると
  読みが `のてんき` になり、先頭一致では引けない。文節の終わりでスペースを
  押す手順を本文で明示する。第 4 段階の自動確定後は `kyou␣no...` で
  `今日` が確定し、次の未確定が `の...` から始まる。
- 単独の `n` や未完成の子音は、既存仕様どおりその文字のまま確定する。
- 全角スペースはコマンド引数の ASCII 空白区切りとは異なる。IME 有効中の
  コマンド入力の使い勝手は文章入力とは別に検討する。
- `lookup_font_16x16` のキー生成にある `c as u16` は補助平面の文字を別の
  文字に誤対応させうる。本計画とは別の小修正として `c as u32` に直す。
- `JP_FONT_CACHE` の `static mut` の並行アクセスは既存の別課題として扱う。

## 完了条件

- 既存のローマ字変換テストと各段階で追加した `#[test_case]` が通る。
- 第 3 段階で、文節ごとの Space と Enter により目標文を入力できる。
- 第 4 段階で、途中の Enter を省略して同じ目標文を入力できる。
- 確定、候補取消、履歴呼び出し、IME オフの後に下線や変換状態が残らない。
- 未確定の最後の文字を消す Backspace が確定済み部分まで消さない。
- VRAM での描画確認と、シリアル端末での下線表示確認を別々に行う。
