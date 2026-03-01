* .comm / .xref / .xdef directives
	.xdef	exported_sym
	.xref	imported_sym
	.text
exported_sym:
	move.l	imported_sym,d0
	nop
	.comm	common_area,256
	.end
