* Relative section directives
	.text
	nop
* Switch to relative data
	.rdata
rdata_sym:
	.dc.l	$12345678
* Back to text
	.text
	nop
* Relative BSS
	.rbss
rbss_sym:
	.ds.l	4
* Relative stack
	.rstack
rstack_sym:
	.ds.l	8
* Back to text
	.text
	nop
	.end
