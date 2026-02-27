* Expression operator tests
* Arithmetic, comparison, shift, unary operators
	.text

* ─── basic arithmetic ────────────────────────────────────────────────────────
	.dc.w	1+2
	.dc.w	10-3
	.dc.w	3*4
	.dc.w	10/3

* ─── operator precedence ─────────────────────────────────────────────────────
	.dc.w	2+3*4
	.dc.w	(2+3)*4
	.dc.w	10-2*3

* ─── shift operators ─────────────────────────────────────────────────────────
	.dc.w	1<<4
	.dc.w	$10>>2
	.dc.l	1<<16
	.dc.l	$10000>>8

* ─── comparison operators (true=-1/0xFFFF, false=0) ─────────────────────────
	.dc.w	1=1
	.dc.w	1=2
	.dc.w	1<>2
	.dc.w	1<>1
	.dc.w	1<2
	.dc.w	2<1
	.dc.w	2>1
	.dc.w	1>2
	.dc.w	1<=1
	.dc.w	2<=1
	.dc.w	1>=1
	.dc.w	1>=2

* ─── unary operators ─────────────────────────────────────────────────────────
	.dc.w	-1
	.dc.w	-100
	.dc.l	-1
	.dc.w	.not.0
	.dc.w	.not.$FF
	.dc.l	.not.0

* ─── symbol-based expressions ────────────────────────────────────────────────
BASE	.equ	$1000
OFFSET	.equ	$20

	.dc.w	BASE
	.dc.w	BASE+OFFSET
	.dc.w	BASE-OFFSET
	.dc.w	BASE>>4
	.dc.w	OFFSET*2

* ─── nested parentheses ──────────────────────────────────────────────────────
	.dc.w	((1+2)*(3+4))
	.dc.l	(($FF<<8)+$AB)

* ─── character constants ─────────────────────────────────────────────────────
	.dc.b	'A'
	.dc.b	'Z'
	.dc.b	' '
	.dc.w	'AB'
