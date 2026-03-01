* .rept / .irp / .irpc edge cases
	.text
* .rept 0 should emit nothing
	.rept	0
	.fail	should not execute
	.endm
* .rept 1 should emit once
	.rept	1
	nop
	.endm
* .rept with labels
	.rept	3
	nop
	.endm
* Nested .rept
	.rept	2
	.rept	2
	nop
	.endm
	.endm
* .irp with multiple arguments
	.irp	reg,d0,d1,d2,d3
	clr.l	reg
	.endm
* .irpc with string
	.irpc	ch,ABCD
	.dc.b	'ch'
	.endm
	.even
	.end
