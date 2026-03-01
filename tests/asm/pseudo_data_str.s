* String literal variants in .dc.b
	.text
* Single-quoted strings (character mode)
	.dc.b	'A'
	.dc.b	'AB'
	.dc.b	'ABCD'
* Double-quoted strings (with trailing zero)
	.dc.b	"Hello",0
	.dc.b	"Test string",0
* Mixed
	.dc.b	'X',0,'Y',0
	.dc.b	"Mix",'!',0
* Empty and single char
	.dc.b	' '
	.dc.b	'0','1','2','3'
* Alignment
	.even
	nop
	.end
