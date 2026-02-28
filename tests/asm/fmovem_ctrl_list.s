; fmovem_ctrl_list.s -- FMOVEM FPCR list

	.68040
	.fpid	3

	fmovem.l	fpcr/fpsr,(a0)
	fmovem.l	(a0),fpcr/fpsr
