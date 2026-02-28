	.68040
	.fpid	3

	; no operand
	fnop

	; register to register
	fmove.x	fp0,fp1
	fadd.x	fp2,fp3
	fsub.x	fp3,fp4
	fmul.x	fp4,fp5
	fdiv.x	fp5,fp6
	fcmp.x	fp1,fp2
	ftst.x	fp2

	; EA to FPn
	fmove.l	d0,fp1
	fadd.l	(a0),fp1
	ftst	(a0)

	; FPn to EA
	fmove.x	fp1,(a0)

	; immediate ROM constant
	fmovecr	#1,fp2

	; state frame save / restore
	fsave	(a0)
	frestore	(a0)
