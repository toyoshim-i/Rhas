* MOVE / MOVEA / MOVEQ / LEA / PEA / EXG / MOVEM / MOVEP instruction tests
	.text

* ─── move.b ──────────────────────────────────────────────────────────────────
	move.b	d0,d1
	move.b	d7,d0
	move.b	#0,d0
	move.b	#127,d0
	move.b	#255,d0
	move.b	(a0),d0
	move.b	(a0)+,d0
	move.b	-(a0),d0
	move.b	4(a0),d0
	move.b	-4(a0),d0
	move.b	d0,(a0)
	move.b	d0,(a0)+
	move.b	d0,-(a0)
	move.b	d0,4(a0)
	move.b	(a0),(a1)

* ─── move.w ──────────────────────────────────────────────────────────────────
	move.w	d0,d1
	move.w	a0,d0
	move.w	#0x1234,d0
	move.w	(a0),d0
	move.w	d0,(a0)
	move.w	(a0)+,(a1)+
	move.w	-(a0),-(a1)

* ─── move.l ──────────────────────────────────────────────────────────────────
	move.l	d0,d1
	move.l	a0,d0
	move.l	#0x12345678,d0
	move.l	(a0),d0
	move.l	d0,(a0)
	move.l	(sp)+,d0
	move.l	d0,-(sp)

* ─── move to/from CCR/SR ─────────────────────────────────────────────────────
	move.w	d0,ccr
	move.w	#0,ccr
	move.w	d0,sr
	move.w	sr,d0

* ─── movea ───────────────────────────────────────────────────────────────────
	movea.w	d0,a0
	movea.w	#100,a0
	movea.w	(a1),a0
	movea.l	d0,a0
	movea.l	a1,a0
	movea.l	#0x12345678,a0
	movea.l	(a1),a0

* ─── moveq ───────────────────────────────────────────────────────────────────
	moveq	#0,d0
	moveq	#1,d1
	moveq	#127,d2
	moveq	#-1,d3
	moveq	#-128,d4

* ─── lea ─────────────────────────────────────────────────────────────────────
	lea	(a0),a1
	lea	4(a0),a1
	lea	-4(a0),a1
	lea	0(a0,d0.w),a1
	lea	4(a0,d1.l),a2

* ─── pea ─────────────────────────────────────────────────────────────────────
	pea	(a0)
	pea	4(a0)
	pea	0(a0,d0.w)

* ─── exg ─────────────────────────────────────────────────────────────────────
	exg	d0,d1
	exg	d7,d6
	exg	a0,a1
	exg	a6,a7
	exg	d0,a0
	exg	d3,a3

* ─── movem ───────────────────────────────────────────────────────────────────
	movem.w	d0-d3,-(sp)
	movem.l	d0-d7,-(sp)
	movem.l	d0-d7/a0-a6,-(sp)
	movem.w	(sp)+,d0-d3
	movem.l	(sp)+,d0-d7
	movem.l	(sp)+,d0-d7/a0-a6
	movem.w	d0/d2/d4,-(a0)
	movem.w	(a0)+,d0/d2/d4

* ─── movep ───────────────────────────────────────────────────────────────────
	movep.w	d0,4(a0)
	movep.l	d0,4(a0)
	movep.w	4(a0),d0
	movep.l	4(a0),d0
