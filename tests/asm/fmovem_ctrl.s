; fmovem_ctrl.s -- FMOVEM 制御レジスタ転送

	.68040
	.fpid	3

	fmovem	fpcr,(a0)
	fmovem	fpsr,(a0)
	fmovem	fpiar,(a0)
	fmovem	(a0),fpcr
	fmovem	(a0),fpsr
	fmovem	(a0),fpiar
