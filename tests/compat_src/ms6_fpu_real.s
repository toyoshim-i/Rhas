	.68040
	.fpid	3

	fnop
	fmove.x	fp0,fp1
	fadd.x	fp2,fp3
	fsub.x	fp3,fp4
	fmul.x	fp4,fp5
	fdiv.x	fp5,fp6
	fcmp.x	fp1,fp2
	ftst.x	fp2
	fmove.l	d0,fp1
	fadd.l	(a0),fp1
	fmove.x	fp1,(a0)
	fmovecr	#1,fp2
	fsave	(a0)
	frestore	(a0)
	fmovem	fpcr,(a0)
	fmovem	fpsr,(a0)
	fmovem	fpiar,(a0)
	fmovem	(a0),fpcr
	fmovem	(a0),fpsr
	fmovem	(a0),fpiar
	fmovem.x	fp0/fp1,(a0)
	fmovem.x	(a0),fp0/fp1
	fmovem.x	fp0/fp1,-(a0)
	fmovem.x	(a0)+,fp0/fp1
	fmovem.x	d0,(a0)
	fmovem.x	(a0),d0
	fmovem.x	d0,-(a0)
	fmovem.x	(a0)+,d0
