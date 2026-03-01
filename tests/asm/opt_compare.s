* Source for comparing default vs -c4 optimization output
* This file should produce different binary output depending on optimization level
	.text
* CLR.L Dn → MOVEQ #0,Dn (c4 opt)
	clr.l	d0
	clr.l	d1
* MOVEA.L #imm,An where imm fits in word → MOVEA.W (c4 opt)
	movea.l	#100,a0
	movea.l	#-1,a1
* CMPI.W #0,Dn → TST.W Dn (c4 opt)
	cmpi.w	#0,d0
	cmpi.l	#0,d1
* LEA d(An),An where d fits in ADDQ → ADDQ (c4 opt)
	lea	4(a0),a0
	lea	8(a1),a1
* ASL.W #1,Dn → ADD.W Dn,Dn (c4 opt)
	asl.w	#1,d0
	asl.l	#1,d1
* SUB #0 / ADD #0 → removed (c4 opt)
	subi.l	#0,d0
	addi.w	#0,d1
	.end
