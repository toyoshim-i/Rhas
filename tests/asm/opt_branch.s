* Branch optimization edge cases (default mode)
	.text
* Forward branch — should be optimized to .s when possible
	bra.w	near
	nop
near:
	nop
* Backward branch
	bra.w	near
* Branch to self (distance = 0, should become .s or cut)
here:	bra.w	here
* Conditional branches
	beq.w	near
	bne.w	near
* DBcc (fixed 16-bit displacement)
	dbra	d0,near
	dbeq	d1,near
	.end
