; addq_opt.s -- ADD/SUB #imm(1-8) → ADDQ/SUBQ 最適化のテスト
;
; -c4 (opt_adda_suba フラグ) が有効のとき、
; ADD.x #N,<ea> (N=1-8) は ADDQ.x #N,<ea> にコンパイルされる。
; SUB.x #N,<ea> (N=1-8) は SUBQ.x #N,<ea> にコンパイルされる。

	.xref	EXTSYM
	.text

	; ADD #imm(1-8), Dn → ADDQ
	add.b	#1,d0
	add.w	#4,d1
	add.l	#8,d2

	; ADD #imm(1-8), (An) → ADDQ
	add.l	#4,(a0)

	; ADD #imm(1-8), (d,An) でオフセット付き外部参照 → ADDQ + ROFST
	add.l	#4,(EXTSYM+10,a0)

	; SUB #imm(1-8), Dn → SUBQ
	sub.b	#1,d3
	sub.w	#3,d4
	sub.l	#7,d5

	; ADD #9 は範囲外なので ADDI のまま
	add.l	#9,d0
