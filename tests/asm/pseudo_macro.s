* Macro pseudo-instruction tests
* .macro / .endm / .rept / .irp / .irpc
	.text

* ─── no-argument macro ───────────────────────────────────────────────────────
nop_twice	.macro
	nop
	nop
	.endm

	nop_twice
	nop_twice

* ─── 1-argument macro ────────────────────────────────────────────────────────
push_d	.macro	reg
	move.l	&reg,-(sp)
	.endm

	push_d	d0
	push_d	d1
	push_d	d7

* ─── 2-argument macro ────────────────────────────────────────────────────────
load_const	.macro	reg,val
	moveq	#&val,&reg
	.endm

	load_const	d0,0
	load_const	d1,42
	load_const	d2,-1

* ─── macro with local labels ─────────────────────────────────────────────────
delay_loop	.macro	cnt
	moveq	#&cnt,d7
@loop	dbra	d7,@loop
	.endm

	delay_loop	10
	delay_loop	99

* ─── .rept: fixed count repeat ───────────────────────────────────────────────
	.rept	3
	nop
	.endm

	.rept	4
	.dc.b	0xFF
	.endm

* ─── .irp: iterate over argument list ───────────────────────────────────────
	.irp	reg,d0,d1,d2,d3
	moveq	#0,&reg
	.endm

	.irp	val,1,2,4,8
	.dc.w	&val
	.endm

* ─── .irpc: iterate over characters ─────────────────────────────────────────
	.irpc	c,abc
	.dc.b	'&c'
	.endm

	.irpc	c,XYZ
	.dc.b	'&c'
	.endm
