* Advanced macro features: nesting, rept, irp
	.text
* Basic nested macro call
pushreg	.macro	reg
	move.l	\reg,-(sp)
	.endm
popreg	.macro	reg
	move.l	(sp)+,\reg
	.endm
saveregs	.macro
	pushreg	d0
	pushreg	d1
	pushreg	a0
	.endm
restregs	.macro
	popreg	a0
	popreg	d1
	popreg	d0
	.endm
	saveregs
	restregs
* .rept
	.rept	3
	nop
	.endm
* .irp
	.irp	reg,d0,d1,d2
	clr.l	reg
	.endm
* .irpc
	.irpc	ch,ABC
	dc.b	'ch'
	.endm
	.even
	.end
