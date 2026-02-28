; fsincos.s -- FSINCOS encoding
	.68040
	.fpid	3

	fsincos.x	fp0,fp1:fp2
	fsincos.x	(a0),fp1:fp2
	fsincos.l	d0,fp3:fp4
	fsincos.x	fp0,fp5:fp6
	fsincos.x	fp3,fp1:fp2
	fsincos.x	fp0,fp0:fp1
	fsincos.x	fp0,fp1:fp0
