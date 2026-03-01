* TRAPcc conditional trap instructions (68020+)
	.cpu	68020
	.text
* No operand forms (unsized)
	trapt
	trapf
* Word immediate forms
	traphi.w	#1
	trapls.w	#2
	trapcc.w	#$100
	trapcs.w	#0
	trapne.w	#$FFFF
	trapeq.w	#$1234
* Long immediate forms
	trapvc.l	#$12345678
	trapvs.l	#0
	trappl.l	#$FFFFFFFF
	trapmi.l	#1
	trapge.l	#$7FFFFFFF
	traplt.l	#$80000000
	trapgt.l	#$ABCDEF01
	traple.l	#$DEADBEEF
	.end
