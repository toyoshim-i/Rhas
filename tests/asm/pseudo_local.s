* Numeric local labels
	.text
1:
	nop
	bra.s	1b
1:
	nop
	bra.s	1b
* Multiple local label numbers
2:
	nop
3:
	nop
	bra.s	2b
	bra.s	3b
	.end
