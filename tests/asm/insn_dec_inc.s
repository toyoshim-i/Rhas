* DEC / INC instructions (HAS-specific SUBQ/ADDQ #1)
	.text
* INC variants
	inc.b	d0
	inc.w	d1
	inc.l	d2
	inc.w	a0
	inc.w	(a0)
	inc.l	(a0)+
* DEC variants
	dec.b	d3
	dec.w	d4
	dec.l	d5
	dec.w	a1
	dec.w	(a1)
	dec.l	-(a1)
	.end
