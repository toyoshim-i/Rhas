* SCD debug output with .file directive
	.file	"test.c"
	.text
	.def	_main
	.val	_main
	.scl	2
	.type	$24
	.endef
_main:
	nop
	rts
	.def	.ef
	.val	.
	.scl	-1
	.ln	5
	.endef
	.end
