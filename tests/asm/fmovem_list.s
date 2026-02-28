; fmovem_list.s -- FMOVEM FPn static list

	.68040
	.fpid	3

	fmovem.x	fp0/fp1,(a0)
	fmovem.x	(a0),fp0/fp1
	fmovem.x	fp0/fp1,-(a0)
	fmovem.x	(a0)+,fp0/fp1
