; c4_core_opt.s -- -c4 拡張最適化の主要ケース
;
; 目的:
; - addq_opt.s で未カバーだった -c4 の代表最適化を
;   HAS060.X ゴールデン比較で固定する。

	.68040
	.text

	; OPTCLR: CLR.L Dn -> MOVEQ #0,Dn
	clr.l	d0

	; OPTMOVE0: MOVE.B/W #0,Dn -> CLR.B/W Dn
	move.b	#0,d1

	; OPTCMPI0: CMPI #0,<ea> -> TST <ea>
	cmpi.l	#0,d2

	; OPTSUBADDI0: ADDI/SUBI #1..8,<ea> -> ADDQ/SUBQ
	addi.w	#3,d3
	subi.l	#2,d4

	; OPTMOVEA: MOVEA.L #imm16,An -> MOVEA.W #imm,An
	movea.l	#1234,a2

	; OPTCMPA: CMPA #0,An -> TST.L An (68020+)
	cmpa.l	#0,a2

	; OPTLEA: LEA (An),An 削除 / LEA (d,An),An -> ADDQ/SUBQ
	lea	(a3),a3
	lea	(4,a4),a4

	; OPTASL: ASL #1,Dn -> ADD Dn,Dn
	asl.w	#1,d5

	; OPTJMPJSR: JMP/JSR label -> JBRA/JBSR 系
	jmp	j_tgt
	jsr	s_tgt

	; OPTBSR: 直後 BSR -> PEA (2,PC)
	bsr	b_tgt

j_tgt:
	nop
s_tgt:
	rts
b_tgt:
	nop
