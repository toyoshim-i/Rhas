* Expression operators: bitwise, shift, unary
	.text
* Bitwise AND
	dc.l	$FF00&$0F0F
	dc.l	$AAAA&$5555
* Bitwise OR
	dc.l	$FF00|$00FF
	dc.l	$A000|$0B00
* Bitwise XOR
	dc.l	$FF00^$0F0F
	dc.l	$AAAA^$5555
* Bitwise NOT (unary)
	dc.l	~0
	dc.l	~$FF
* Unary minus
	dc.l	-1
	dc.l	-$80000000
* Shift operators in expressions
	dc.l	1<<8
	dc.l	$FF00>>4
* Mixed
	dc.l	($FF&$0F)|($F0&$F0)
	dc.l	(1<<16)-1
	.end
