* MOVEM instruction variants
	.text
* Register to memory
	movem.w	d0-d7,(a0)
	movem.l	d0-d7/a0-a6,(a1)
	movem.l	d0/d2/d4,-(sp)
	movem.w	a0-a3,-(sp)
* Memory to register
	movem.w	(a0),d0-d7
	movem.l	(sp)+,d0-d7/a0-a6
	movem.l	(sp)+,d0/d2/d4
	movem.w	(sp)+,a0-a3
* Single register
	movem.l	d0,(a0)
	movem.l	(a0),d0
* Displacement
	movem.l	d0-d3,100(a0)
	movem.l	100(a0),d0-d3
	.end
