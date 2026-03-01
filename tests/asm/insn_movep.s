* MOVEP instruction
	.text
	movep.w	d0,0(a0)
	movep.l	d1,4(a0)
	movep.w	0(a0),d2
	movep.l	4(a0),d3
	movep.w	d4,100(a1)
	movep.l	d5,-2(a2)
	.end
