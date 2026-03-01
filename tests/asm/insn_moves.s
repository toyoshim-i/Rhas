* MOVES (68010+, privileged)
	.cpu	68010
	.text
	moves.b	d0,(a0)
	moves.w	d1,(a1)
	moves.l	d2,(a2)
	moves.b	(a0),d3
	moves.w	(a1),d4
	moves.l	(a2),d5
	moves.l	a0,(a3)
	moves.l	(a4),a1
	.end
