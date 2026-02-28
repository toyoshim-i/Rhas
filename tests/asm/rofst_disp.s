; rofst_disp.s -- ROFST (外部参照+定数オフセット) ディスプレースメントのテスト
;
; パターン1: (ext+const, An)  → [SymbolRef(ext), Value(const), Add, End]  (既存で動作)
; パターン2: (const+ext, An)  → [Value(const), SymbolRef(ext), Add, End]  (修正対象)
;
; is_external_with_offset が両パターンを ROFST レコードとして出力することを確認する。

	.xref	EXTSYM
	.text

	; パターン1: ext + const (EXTSYM+10, a0)
	move.l	(EXTSYM+10,a0),d0

	; パターン2: const + ext (10+EXTSYM, a1) - これが修正対象
	move.l	(10+EXTSYM,a1),d1

	; サブ (減算): ext - const
	move.l	(EXTSYM-4,a0),d2
