* .offset / .org pseudo-instructions
	.text
start:
	nop
	nop
* .offset creates an absolute offset section
	.offset	0
field1:	.ds.b	4
field2:	.ds.w	1
field3:	.ds.l	1
structsize = *
	.text
* Use offset symbols
	move.b	field1(a0),d0
	move.w	field2(a0),d1
	move.l	field3(a0),d2
	move.l	#structsize,d3
	.end
