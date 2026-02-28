; fmovem_dyn.s -- FMOVEM FPn dynamic list (Dn mask)

	.68040
	.fpid	3

	fmovem.x	d0,(a0)
	fmovem.x	(a0),d0
	fmovem.x	d0,-(a0)
	fmovem.x	(a0)+,d0
