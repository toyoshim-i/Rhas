* Conditional assembly pseudo-instruction tests
* .if / .ifdef / .ifndef / .else / .elseif / .endif
	.text

* ─── .if true / .if false ───────────────────────────────────────────────────
	.if	1
	nop
	.endif

	.if	0
	add.w	d1,d2
	.endif

* ─── .if with expressions ────────────────────────────────────────────────────
	.if	1+1
	move.b	d0,d1
	.endif

	.if	1-1
	move.w	d0,d1
	.endif

	.if	2*3-6
	move.l	d0,d1
	.endif

* ─── .if with comparisons ────────────────────────────────────────────────────
	.if	2>1
	nop
	.endif

	.if	1>2
	nop
	.endif

* ─── .if / .else / .endif ────────────────────────────────────────────────────
	.if	1
	move.b	d0,d1
	.else
	move.w	d0,d1
	.endif

	.if	0
	move.b	d0,d1
	.else
	move.l	d0,d1
	.endif

* ─── .ifdef / .ifndef ────────────────────────────────────────────────────────
DEFINED_SYM	.equ	99

	.ifdef	DEFINED_SYM
	moveq	#1,d0
	.endif

	.ifdef	UNDEFINED_SYM
	moveq	#0,d0
	.endif

	.ifndef	UNDEFINED_SYM
	moveq	#2,d0
	.endif

	.ifndef	DEFINED_SYM
	moveq	#3,d0
	.endif

* ─── nested conditionals ─────────────────────────────────────────────────────
	.if	1
	.if	1
	nop
	.endif
	.if	0
	nop
	.endif
	.endif

* ─── .elseif ─────────────────────────────────────────────────────────────────
LEVEL	.equ	2

	.if	LEVEL=1
	moveq	#1,d0
	.elseif	LEVEL=2
	moveq	#2,d0
	.elseif	LEVEL=3
	moveq	#3,d0
	.else
	moveq	#0,d0
	.endif
