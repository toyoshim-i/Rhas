* Effective address mode tests
* Uses MOVE.L/W/B to exercise all EA modes as src and dst
	.text

* ─── Dn: data register direct ───────────────────────────────────────────────
	move.l	d0,d1
	move.l	d7,d0
	move.b	d3,d5
	move.w	d6,d2

* ─── (An): address register indirect ────────────────────────────────────────
	move.l	(a0),d0
	move.l	(a7),d0
	move.l	d0,(a0)
	move.l	d0,(a7)
	move.w	(a1),d1
	move.w	d1,(a1)

* ─── (An)+: postincrement ────────────────────────────────────────────────────
	move.b	(a0)+,d0
	move.w	(a0)+,d0
	move.l	(a0)+,d0
	move.b	(a7)+,d0
	move.b	d0,(a1)+
	move.l	d0,(a1)+

* ─── -(An): predecrement ─────────────────────────────────────────────────────
	move.b	-(a0),d0
	move.w	-(a0),d0
	move.l	-(a0),d0
	move.b	d0,-(a1)
	move.l	d0,-(a1)
	move.l	d0,-(a7)

* ─── (d16,An): displacement (16-bit) ────────────────────────────────────────
	move.l	0(a0),d0
	move.l	4(a0),d0
	move.l	-4(a0),d0
	move.l	100(a0),d0
	move.l	-100(a0),d0
	move.l	32767(a0),d0
	move.l	-32768(a0),d0
	move.l	d0,4(a1)
	move.w	2(a6),d3

* ─── (d8,An,Rn): index with 8-bit displacement ───────────────────────────────
	move.l	0(a0,d0.w),d1
	move.l	0(a0,d0.l),d1
	move.l	4(a0,d0.w),d1
	move.l	-4(a0,d0.w),d1
	move.l	0(a0,a1.w),d2
	move.l	0(a0,a1.l),d2
	move.l	0(a0,d7.l),d1
	move.l	d0,4(a1,d1.w)

* ─── (xxx).w: absolute short ────────────────────────────────────────────────
	move.w	0x0100.w,d0
	move.w	d0,0x0100.w
	move.l	0x7FFE.w,d0

* ─── (xxx).l: absolute long ─────────────────────────────────────────────────
	move.l	0x00C00000.l,d0
	move.l	d0,0x00C00000.l

* ─── #imm: immediate ────────────────────────────────────────────────────────
	move.b	#0,d0
	move.b	#255,d0
	move.w	#0x1234,d0
	move.l	#0x12345678,d0
	move.w	#0,d0
	moveq	#0,d0

* ─── (d16,PC): PC relative ──────────────────────────────────────────────────
pctgt	nop
	move.l	pctgt(pc),d0
	move.w	pctgt(pc),d0

* ─── (d8,PC,Rn): PC relative with index ─────────────────────────────────────
	move.l	pctgt(pc,d0.w),d1
	move.l	pctgt(pc,d1.l),d2
