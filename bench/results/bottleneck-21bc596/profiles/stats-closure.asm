
target/bottleneck-21bc596/before/rust-fx:     file format elf64-x86-64


Disassembly of section .text:

00000000002dce50 <rust_fx::commands::collect_stats::{closure#1}>:
  2dce50:	55                   	push   %rbp
  2dce51:	41 57                	push   %r15
  2dce53:	41 56                	push   %r14
  2dce55:	41 55                	push   %r13
  2dce57:	41 54                	push   %r12
  2dce59:	53                   	push   %rbx
  2dce5a:	48 81 ec 98 00 00 00 	sub    $0x98,%rsp
  2dce61:	48 8b 0f             	mov    (%rdi),%rcx
  2dce64:	4c 8b 56 20          	mov    0x20(%rsi),%r10
  2dce68:	48 8b 6e 28          	mov    0x28(%rsi),%rbp
  2dce6c:	80 39 00             	cmpb   $0x0,(%rcx)
  2dce6f:	0f 84 69 02 00 00    	je     2dd0de <rust_fx::commands::collect_stats::{closure#1}+0x28e>
  2dce75:	48 8d 15 99 5b dc ff 	lea    -0x23a467(%rip),%rdx        # a2a15 <regex_automata::util::pool::inner::THREAD_ID_DROPPED+0x2f95>
  2dce7c:	41 b8 07 00 00 00    	mov    $0x7,%r8d
  2dce82:	48 85 ed             	test   %rbp,%rbp
  2dce85:	0f 84 45 02 00 00    	je     2dd0d0 <rust_fx::commands::collect_stats::{closure#1}+0x280>
  2dce8b:	48 89 4c 24 40       	mov    %rcx,0x40(%rsp)
  2dce90:	48 89 74 24 20       	mov    %rsi,0x20(%rsp)
  2dce95:	49 b8 80 80 80 80 80 	movabs $0x8080808080808080,%r8
  2dce9c:	80 80 80 
  2dce9f:	49 bf 00 01 01 01 01 	movabs $0x101010101010100,%r15
  2dcea6:	01 01 01 
  2dcea9:	4c 89 d1             	mov    %r10,%rcx
  2dceac:	49 01 ea             	add    %rbp,%r10
  2dceaf:	48 8d 15 11 97 d6 ff 	lea    -0x2968ef(%rip),%rdx        # 465c7 <GCC_except_table2456+0x2523>
  2dceb6:	48 83 e2 f8          	and    $0xfffffffffffffff8,%rdx
  2dceba:	48 8d 1d ff 96 d6 ff 	lea    -0x296901(%rip),%rbx        # 465c0 <GCC_except_table2456+0x251c>
  2dcec1:	49 89 d3             	mov    %rdx,%r11
  2dcec4:	49 29 db             	sub    %rbx,%r11
  2dcec7:	48 89 4c 24 10       	mov    %rcx,0x10(%rsp)
  2dcecc:	4c 39 d1             	cmp    %r10,%rcx
  2dcecf:	75 24                	jne    2dcef5 <rust_fx::commands::collect_stats::{closure#1}+0xa5>
  2dced1:	e9 26 01 00 00       	jmp    2dcffc <rust_fx::commands::collect_stats::{closure#1}+0x1ac>
  2dced6:	66 2e 0f 1f 84 00 00 	cs nopw 0x0(%rax,%rax,1)
  2dcedd:	00 00 00 
  2dcee0:	4c 89 e1             	mov    %r12,%rcx
  2dcee3:	44 38 ee             	cmp    %r13b,%sil
  2dcee6:	0f 85 88 01 00 00    	jne    2dd074 <rust_fx::commands::collect_stats::{closure#1}+0x224>
  2dceec:	4c 39 d1             	cmp    %r10,%rcx
  2dceef:	0f 84 07 01 00 00    	je     2dcffc <rust_fx::commands::collect_stats::{closure#1}+0x1ac>
  2dcef5:	4c 8d 61 01          	lea    0x1(%rcx),%r12
  2dcef9:	44 0f b6 29          	movzbl (%rcx),%r13d
  2dcefd:	48 39 da             	cmp    %rbx,%rdx
  2dcf00:	75 0e                	jne    2dcf10 <rust_fx::commands::collect_stats::{closure#1}+0xc0>
  2dcf02:	45 31 f6             	xor    %r14d,%r14d
  2dcf05:	eb 2e                	jmp    2dcf35 <rust_fx::commands::collect_stats::{closure#1}+0xe5>
  2dcf07:	66 0f 1f 84 00 00 00 	nopw   0x0(%rax,%rax,1)
  2dcf0e:	00 00 
  2dcf10:	31 c0                	xor    %eax,%eax
  2dcf12:	66 66 66 66 66 2e 0f 	data16 data16 data16 data16 cs nopw 0x0(%rax,%rax,1)
  2dcf19:	1f 84 00 00 00 00 00 
  2dcf20:	44 38 2c 18          	cmp    %r13b,(%rax,%rbx,1)
  2dcf24:	0f 84 c6 00 00 00    	je     2dcff0 <rust_fx::commands::collect_stats::{closure#1}+0x1a0>
  2dcf2a:	48 ff c0             	inc    %rax
  2dcf2d:	49 39 c3             	cmp    %rax,%r11
  2dcf30:	75 ee                	jne    2dcf20 <rust_fx::commands::collect_stats::{closure#1}+0xd0>
  2dcf32:	4d 89 de             	mov    %r11,%r14
  2dcf35:	4c 89 e9             	mov    %r13,%rcx
  2dcf38:	49 8d 47 01          	lea    0x1(%r15),%rax
  2dcf3c:	48 0f af c8          	imul   %rax,%rcx
  2dcf40:	49 8b 04 1e          	mov    (%r14,%rbx,1),%rax
  2dcf44:	48 31 c8             	xor    %rcx,%rax
  2dcf47:	4c 89 fe             	mov    %r15,%rsi
  2dcf4a:	48 29 c6             	sub    %rax,%rsi
  2dcf4d:	48 09 c6             	or     %rax,%rsi
  2dcf50:	49 8b 44 1e 08       	mov    0x8(%r14,%rbx,1),%rax
  2dcf55:	48 31 c8             	xor    %rcx,%rax
  2dcf58:	4d 89 f9             	mov    %r15,%r9
  2dcf5b:	49 29 c1             	sub    %rax,%r9
  2dcf5e:	49 09 c1             	or     %rax,%r9
  2dcf61:	4c 21 c6             	and    %r8,%rsi
  2dcf64:	4c 21 ce             	and    %r9,%rsi
  2dcf67:	4c 39 c6             	cmp    %r8,%rsi
  2dcf6a:	75 44                	jne    2dcfb0 <rust_fx::commands::collect_stats::{closure#1}+0x160>
  2dcf6c:	49 8d 76 10          	lea    0x10(%r14),%rsi
  2dcf70:	4d 85 f6             	test   %r14,%r14
  2dcf73:	75 2d                	jne    2dcfa2 <rust_fx::commands::collect_stats::{closure#1}+0x152>
  2dcf75:	48 8b 04 1e          	mov    (%rsi,%rbx,1),%rax
  2dcf79:	48 31 c8             	xor    %rcx,%rax
  2dcf7c:	4d 89 f9             	mov    %r15,%r9
  2dcf7f:	49 29 c1             	sub    %rax,%r9
  2dcf82:	49 09 c1             	or     %rax,%r9
  2dcf85:	48 33 4c 1e 08       	xor    0x8(%rsi,%rbx,1),%rcx
  2dcf8a:	4c 89 f8             	mov    %r15,%rax
  2dcf8d:	48 29 c8             	sub    %rcx,%rax
  2dcf90:	48 09 c8             	or     %rcx,%rax
  2dcf93:	4d 21 c1             	and    %r8,%r9
  2dcf96:	49 21 c1             	and    %rax,%r9
  2dcf99:	4d 39 c1             	cmp    %r8,%r9
  2dcf9c:	0f 84 d2 00 00 00    	je     2dd074 <rust_fx::commands::collect_stats::{closure#1}+0x224>
  2dcfa2:	4c 8d 34 33          	lea    (%rbx,%rsi,1),%r14
  2dcfa6:	b9 20 00 00 00       	mov    $0x20,%ecx
  2dcfab:	48 29 f1             	sub    %rsi,%rcx
  2dcfae:	eb 13                	jmp    2dcfc3 <rust_fx::commands::collect_stats::{closure#1}+0x173>
  2dcfb0:	4c 89 f1             	mov    %r14,%rcx
  2dcfb3:	48 83 e9 20          	sub    $0x20,%rcx
  2dcfb7:	0f 84 b7 00 00 00    	je     2dd074 <rust_fx::commands::collect_stats::{closure#1}+0x224>
  2dcfbd:	49 01 de             	add    %rbx,%r14
  2dcfc0:	48 f7 d9             	neg    %rcx
  2dcfc3:	48 ff c9             	dec    %rcx
  2dcfc6:	31 c0                	xor    %eax,%eax
  2dcfc8:	0f 1f 84 00 00 00 00 	nopl   0x0(%rax,%rax,1)
  2dcfcf:	00 
  2dcfd0:	41 0f b6 34 06       	movzbl (%r14,%rax,1),%esi
  2dcfd5:	44 38 ee             	cmp    %r13b,%sil
  2dcfd8:	0f 84 02 ff ff ff    	je     2dcee0 <rust_fx::commands::collect_stats::{closure#1}+0x90>
  2dcfde:	48 39 c1             	cmp    %rax,%rcx
  2dcfe1:	48 8d 40 01          	lea    0x1(%rax),%rax
  2dcfe5:	75 e9                	jne    2dcfd0 <rust_fx::commands::collect_stats::{closure#1}+0x180>
  2dcfe7:	e9 f4 fe ff ff       	jmp    2dcee0 <rust_fx::commands::collect_stats::{closure#1}+0x90>
  2dcfec:	0f 1f 40 00          	nopl   0x0(%rax)
  2dcff0:	4c 89 e1             	mov    %r12,%rcx
  2dcff3:	4c 39 d1             	cmp    %r10,%rcx
  2dcff6:	0f 85 f9 fe ff ff    	jne    2dcef5 <rust_fx::commands::collect_stats::{closure#1}+0xa5>
  2dcffc:	31 c0                	xor    %eax,%eax
  2dcffe:	4c 8b 54 24 10       	mov    0x10(%rsp),%r10
  2dd003:	66 66 66 66 2e 0f 1f 	data16 data16 data16 cs nopw 0x0(%rax,%rax,1)
  2dd00a:	84 00 00 00 00 00 
  2dd010:	48 89 c1             	mov    %rax,%rcx
  2dd013:	48 39 c5             	cmp    %rax,%rbp
  2dd016:	74 11                	je     2dd029 <rust_fx::commands::collect_stats::{closure#1}+0x1d9>
  2dd018:	41 0f b6 14 0a       	movzbl (%r10,%rcx,1),%edx
  2dd01d:	80 c2 ac             	add    $0xac,%dl
  2dd020:	48 8d 41 01          	lea    0x1(%rcx),%rax
  2dd024:	f6 c2 df             	test   $0xdf,%dl
  2dd027:	75 e7                	jne    2dd010 <rust_fx::commands::collect_stats::{closure#1}+0x1c0>
  2dd029:	31 c0                	xor    %eax,%eax
  2dd02b:	48 8b 74 24 20       	mov    0x20(%rsp),%rsi
  2dd030:	48 89 c2             	mov    %rax,%rdx
  2dd033:	48 39 c5             	cmp    %rax,%rbp
  2dd036:	74 13                	je     2dd04b <rust_fx::commands::collect_stats::{closure#1}+0x1fb>
  2dd038:	45 0f b6 04 12       	movzbl (%r10,%rdx,1),%r8d
  2dd03d:	41 80 c0 ab          	add    $0xab,%r8b
  2dd041:	48 8d 42 01          	lea    0x1(%rdx),%rax
  2dd045:	41 f6 c0 df          	test   $0xdf,%r8b
  2dd049:	75 e5                	jne    2dd030 <rust_fx::commands::collect_stats::{closure#1}+0x1e0>
  2dd04b:	48 39 d5             	cmp    %rdx,%rbp
  2dd04e:	48 8d 05 fd 5d dc ff 	lea    -0x23a203(%rip),%rax        # a2e52 <regex_automata::util::pool::inner::THREAD_ID_DROPPED+0x33d2>
  2dd055:	48 8d 15 f3 5d dc ff 	lea    -0x23a20d(%rip),%rdx        # a2e4f <regex_automata::util::pool::inner::THREAD_ID_DROPPED+0x33cf>
  2dd05c:	48 0f 44 d0          	cmove  %rax,%rdx
  2dd060:	48 39 cd             	cmp    %rcx,%rbp
  2dd063:	48 0f 45 d0          	cmovne %rax,%rdx
  2dd067:	41 b8 03 00 00 00    	mov    $0x3,%r8d
  2dd06d:	48 8b 4c 24 40       	mov    0x40(%rsp),%rcx
  2dd072:	eb 5c                	jmp    2dd0d0 <rust_fx::commands::collect_stats::{closure#1}+0x280>
  2dd074:	31 d2                	xor    %edx,%edx
  2dd076:	48 8d 0d cb 5d dc ff 	lea    -0x23a235(%rip),%rcx        # a2e48 <regex_automata::util::pool::inner::THREAD_ID_DROPPED+0x33c8>
  2dd07d:	4c 8b 54 24 10       	mov    0x10(%rsp),%r10
  2dd082:	48 8b 74 24 20       	mov    0x20(%rsp),%rsi
  2dd087:	eb 0f                	jmp    2dd098 <rust_fx::commands::collect_stats::{closure#1}+0x248>
  2dd089:	0f 1f 80 00 00 00 00 	nopl   0x0(%rax)
  2dd090:	48 ff c2             	inc    %rdx
  2dd093:	48 39 d5             	cmp    %rdx,%rbp
  2dd096:	74 2a                	je     2dd0c2 <rust_fx::commands::collect_stats::{closure#1}+0x272>
  2dd098:	45 0f b6 04 12       	movzbl (%r10,%rdx,1),%r8d
  2dd09d:	44 89 c0             	mov    %r8d,%eax
  2dd0a0:	24 df                	and    $0xdf,%al
  2dd0a2:	04 bf                	add    $0xbf,%al
  2dd0a4:	3c 19                	cmp    $0x19,%al
  2dd0a6:	76 e8                	jbe    2dd090 <rust_fx::commands::collect_stats::{closure#1}+0x240>
  2dd0a8:	41 83 f8 2a          	cmp    $0x2a,%r8d
  2dd0ac:	74 e2                	je     2dd090 <rust_fx::commands::collect_stats::{closure#1}+0x240>
  2dd0ae:	41 83 f8 2d          	cmp    $0x2d,%r8d
  2dd0b2:	74 dc                	je     2dd090 <rust_fx::commands::collect_stats::{closure#1}+0x240>
  2dd0b4:	48 8b 4c 24 40       	mov    0x40(%rsp),%rcx
  2dd0b9:	48 8d 15 55 59 dc ff 	lea    -0x23a6ab(%rip),%rdx        # a2a15 <regex_automata::util::pool::inner::THREAD_ID_DROPPED+0x2f95>
  2dd0c0:	eb 08                	jmp    2dd0ca <rust_fx::commands::collect_stats::{closure#1}+0x27a>
  2dd0c2:	48 89 ca             	mov    %rcx,%rdx
  2dd0c5:	48 8b 4c 24 40       	mov    0x40(%rsp),%rcx
  2dd0ca:	41 b8 07 00 00 00    	mov    $0x7,%r8d
  2dd0d0:	48 8b 47 08          	mov    0x8(%rdi),%rax
  2dd0d4:	48 89 10             	mov    %rdx,(%rax)
  2dd0d7:	4c 89 40 08          	mov    %r8,0x8(%rax)
  2dd0db:	c6 01 00             	movb   $0x0,(%rcx)
  2dd0de:	4c 8b 5f 10          	mov    0x10(%rdi),%r11
  2dd0e2:	4c 8b 47 18          	mov    0x18(%rdi),%r8
  2dd0e6:	4c 8b 77 20          	mov    0x20(%rdi),%r14
  2dd0ea:	41 8b 06             	mov    (%r14),%eax
  2dd0ed:	49 ff 43 10          	incq   0x10(%r11)
  2dd0f1:	49 01 6b 30          	add    %rbp,0x30(%r11)
  2dd0f5:	49 8b 4b 08          	mov    0x8(%r11),%rcx
  2dd0f9:	49 8b 53 38          	mov    0x38(%r11),%rdx
  2dd0fd:	48 39 cd             	cmp    %rcx,%rbp
  2dd100:	48 0f 42 cd          	cmovb  %rbp,%rcx
  2dd104:	41 80 3b 00          	cmpb   $0x0,(%r11)
  2dd108:	48 0f 44 cd          	cmove  %rbp,%rcx
  2dd10c:	48 39 d5             	cmp    %rdx,%rbp
  2dd10f:	48 0f 47 d5          	cmova  %rbp,%rdx
  2dd113:	49 c7 03 01 00 00 00 	movq   $0x1,(%r11)
  2dd11a:	49 89 4b 08          	mov    %rcx,0x8(%r11)
  2dd11e:	49 89 53 38          	mov    %rdx,0x38(%r11)
  2dd122:	83 f8 01             	cmp    $0x1,%eax
  2dd125:	0f 85 b1 03 00 00    	jne    2dd4dc <rust_fx::commands::collect_stats::{closure#1}+0x68c>
  2dd12b:	49 8b 7b 18          	mov    0x18(%r11),%rdi
  2dd12f:	48 85 ff             	test   %rdi,%rdi
  2dd132:	4c 89 44 24 40       	mov    %r8,0x40(%rsp)
  2dd137:	74 6e                	je     2dd1a7 <rust_fx::commands::collect_stats::{closure#1}+0x357>
  2dd139:	4d 8b 43 20          	mov    0x20(%r11),%r8
  2dd13d:	4c 89 c0             	mov    %r8,%rax
  2dd140:	48 89 fb             	mov    %rdi,%rbx
  2dd143:	4c 8d 63 08          	lea    0x8(%rbx),%r12
  2dd147:	44 0f b7 bb ba 00 00 	movzwl 0xba(%rbx),%r15d
  2dd14e:	00 
  2dd14f:	44 89 f9             	mov    %r15d,%ecx
  2dd152:	c1 e1 03             	shl    $0x3,%ecx
  2dd155:	49 c7 c5 ff ff ff ff 	mov    $0xffffffffffffffff,%r13
  2dd15c:	0f 1f 40 00          	nopl   0x0(%rax)
  2dd160:	48 85 c9             	test   %rcx,%rcx
  2dd163:	74 2b                	je     2dd190 <rust_fx::commands::collect_stats::{closure#1}+0x340>
  2dd165:	4a 3b 6c eb 10       	cmp    0x10(%rbx,%r13,8),%rbp
  2dd16a:	0f 97 c2             	seta   %dl
  2dd16d:	80 da 00             	sbb    $0x0,%dl
  2dd170:	49 ff c5             	inc    %r13
  2dd173:	48 83 c1 f8          	add    $0xfffffffffffffff8,%rcx
  2dd177:	80 fa 01             	cmp    $0x1,%dl
  2dd17a:	74 e4                	je     2dd160 <rust_fx::commands::collect_stats::{closure#1}+0x310>
  2dd17c:	0f b6 ca             	movzbl %dl,%ecx
  2dd17f:	85 c9                	test   %ecx,%ecx
  2dd181:	0f 84 82 00 00 00    	je     2dd209 <rust_fx::commands::collect_stats::{closure#1}+0x3b9>
  2dd187:	eb 0a                	jmp    2dd193 <rust_fx::commands::collect_stats::{closure#1}+0x343>
  2dd189:	0f 1f 80 00 00 00 00 	nopl   0x0(%rax)
  2dd190:	4d 89 fd             	mov    %r15,%r13
  2dd193:	48 83 e8 01          	sub    $0x1,%rax
  2dd197:	0f 82 51 03 00 00    	jb     2dd4ee <rust_fx::commands::collect_stats::{closure#1}+0x69e>
  2dd19d:	4a 8b 9c eb c0 00 00 	mov    0xc0(%rbx,%r13,8),%rbx
  2dd1a4:	00 
  2dd1a5:	eb 9c                	jmp    2dd143 <rust_fx::commands::collect_stats::{closure#1}+0x2f3>
  2dd1a7:	4c 89 54 24 10       	mov    %r10,0x10(%rsp)
  2dd1ac:	4d 89 f4             	mov    %r14,%r12
  2dd1af:	4d 89 df             	mov    %r11,%r15
  2dd1b2:	49 89 f6             	mov    %rsi,%r14
  2dd1b5:	bf c0 00 00 00       	mov    $0xc0,%edi
  2dd1ba:	ff 15 90 77 17 00    	call   *0x177790(%rip)        # 454950 <malloc@GLIBC_2.2.5>
  2dd1c0:	48 85 c0             	test   %rax,%rax
  2dd1c3:	0f 84 bb 0b 00 00    	je     2ddd84 <rust_fx::commands::collect_stats::{closure#1}+0xf34>
  2dd1c9:	48 89 c3             	mov    %rax,%rbx
  2dd1cc:	48 c7 00 00 00 00 00 	movq   $0x0,(%rax)
  2dd1d3:	4d 89 fb             	mov    %r15,%r11
  2dd1d6:	49 89 47 18          	mov    %rax,0x18(%r15)
  2dd1da:	49 c7 47 20 00 00 00 	movq   $0x0,0x20(%r15)
  2dd1e1:	00 
  2dd1e2:	66 c7 80 ba 00 00 00 	movw   $0x1,0xba(%rax)
  2dd1e9:	01 00 
  2dd1eb:	48 89 68 08          	mov    %rbp,0x8(%rax)
  2dd1ef:	48 c7 40 60 00 00 00 	movq   $0x0,0x60(%rax)
  2dd1f6:	00 
  2dd1f7:	45 31 ed             	xor    %r13d,%r13d
  2dd1fa:	4c 89 f6             	mov    %r14,%rsi
  2dd1fd:	4d 89 e6             	mov    %r12,%r14
  2dd200:	4c 8b 54 24 10       	mov    0x10(%rsp),%r10
  2dd205:	49 ff 43 28          	incq   0x28(%r11)
  2dd209:	49 89 f7             	mov    %rsi,%r15
  2dd20c:	4a ff 44 eb 60       	incq   0x60(%rbx,%r13,8)
  2dd211:	48 85 ed             	test   %rbp,%rbp
  2dd214:	74 70                	je     2dd286 <rust_fx::commands::collect_stats::{closure#1}+0x436>
  2dd216:	31 c9                	xor    %ecx,%ecx
  2dd218:	48 8d 3d a9 7d dc ff 	lea    -0x238257(%rip),%rdi        # a4fc8 <core::unicode::unicode_data::white_space::WHITESPACE_MAP+0xfe8>
  2dd21f:	45 31 c0             	xor    %r8d,%r8d
  2dd222:	31 d2                	xor    %edx,%edx
  2dd224:	31 c0                	xor    %eax,%eax
  2dd226:	48 8b 5c 24 40       	mov    0x40(%rsp),%rbx
  2dd22b:	eb 42                	jmp    2dd26f <rust_fx::commands::collect_stats::{closure#1}+0x41f>
  2dd22d:	0f 1f 00             	nopl   (%rax)
  2dd230:	45 0f b6 c9          	movzbl %r9b,%r9d
  2dd234:	45 0f b6 0c 39       	movzbl (%r9,%rdi,1),%r9d
  2dd239:	4c 01 ca             	add    %r9,%rdx
  2dd23c:	44 0f b6 0c 33       	movzbl (%rbx,%rsi,1),%r9d
  2dd241:	4c 01 c9             	add    %r9,%rcx
  2dd244:	44 8d 4e 9f          	lea    -0x61(%rsi),%r9d
  2dd248:	41 80 f9 1a          	cmp    $0x1a,%r9b
  2dd24c:	41 0f 92 c1          	setb   %r9b
  2dd250:	41 c0 e1 05          	shl    $0x5,%r9b
  2dd254:	41 30 f1             	xor    %sil,%r9b
  2dd257:	41 80 c1 bd          	add    $0xbd,%r9b
  2dd25b:	41 80 e1 fb          	and    $0xfb,%r9b
  2dd25f:	41 80 f9 01          	cmp    $0x1,%r9b
  2dd263:	48 83 d0 00          	adc    $0x0,%rax
  2dd267:	49 ff c0             	inc    %r8
  2dd26a:	4c 39 c5             	cmp    %r8,%rbp
  2dd26d:	74 1d                	je     2dd28c <rust_fx::commands::collect_stats::{closure#1}+0x43c>
  2dd26f:	43 0f b6 34 02       	movzbl (%r10,%r8,1),%esi
  2dd274:	44 8d 4e b2          	lea    -0x4e(%rsi),%r9d
  2dd278:	41 d0 c9             	ror    $1,%r9b
  2dd27b:	41 80 f9 15          	cmp    $0x15,%r9b
  2dd27f:	76 af                	jbe    2dd230 <rust_fx::commands::collect_stats::{closure#1}+0x3e0>
  2dd281:	45 31 c9             	xor    %r9d,%r9d
  2dd284:	eb b3                	jmp    2dd239 <rust_fx::commands::collect_stats::{closure#1}+0x3e9>
  2dd286:	31 c0                	xor    %eax,%eax
  2dd288:	31 d2                	xor    %edx,%edx
  2dd28a:	31 c9                	xor    %ecx,%ecx
  2dd28c:	49 01 4b 40          	add    %rcx,0x40(%r11)
  2dd290:	49 01 53 48          	add    %rdx,0x48(%r11)
  2dd294:	49 01 43 50          	add    %rax,0x50(%r11)
  2dd298:	49 8b 47 48          	mov    0x48(%r15),%rax
  2dd29c:	49 8b 4f 38          	mov    0x38(%r15),%rcx
  2dd2a0:	48 09 c1             	or     %rax,%rcx
  2dd2a3:	0f 84 33 02 00 00    	je     2dd4dc <rust_fx::commands::collect_stats::{closure#1}+0x68c>
  2dd2a9:	48 85 c0             	test   %rax,%rax
  2dd2ac:	74 26                	je     2dd2d4 <rust_fx::commands::collect_stats::{closure#1}+0x484>
  2dd2ae:	49 8b 4f 40          	mov    0x40(%r15),%rcx
  2dd2b2:	41 0f b7 be 08 08 00 	movzwl 0x808(%r14),%edi
  2dd2b9:	00 
  2dd2ba:	45 0f b7 86 0a 08 00 	movzwl 0x80a(%r14),%r8d
  2dd2c1:	00 
  2dd2c2:	48 83 f8 04          	cmp    $0x4,%rax
  2dd2c6:	73 19                	jae    2dd2e1 <rust_fx::commands::collect_stats::{closure#1}+0x491>
  2dd2c8:	45 31 c9             	xor    %r9d,%r9d
  2dd2cb:	31 d2                	xor    %edx,%edx
  2dd2cd:	31 f6                	xor    %esi,%esi
  2dd2cf:	e9 4c 01 00 00       	jmp    2dd420 <rust_fx::commands::collect_stats::{closure#1}+0x5d0>
  2dd2d4:	31 f6                	xor    %esi,%esi
  2dd2d6:	66 0f 57 c0          	xorpd  %xmm0,%xmm0
  2dd2da:	31 d2                	xor    %edx,%edx
  2dd2dc:	e9 e3 01 00 00       	jmp    2dd4c4 <rust_fx::commands::collect_stats::{closure#1}+0x674>
  2dd2e1:	49 89 c1             	mov    %rax,%r9
  2dd2e4:	49 83 e1 fc          	and    $0xfffffffffffffffc,%r9
  2dd2e8:	66 0f 6e c7          	movd   %edi,%xmm0
  2dd2ec:	f2 0f 70 c0 00       	pshuflw $0x0,%xmm0,%xmm0
  2dd2f1:	66 41 0f 6e c8       	movd   %r8d,%xmm1
  2dd2f6:	f2 0f 70 c9 00       	pshuflw $0x0,%xmm1,%xmm1
  2dd2fb:	66 0f ef d2          	pxor   %xmm2,%xmm2
  2dd2ff:	31 d2                	xor    %edx,%edx
  2dd301:	66 0f 70 c0 00       	pshufd $0x0,%xmm0,%xmm0
  2dd306:	f3 0f 70 e8 55       	pshufhw $0x55,%xmm0,%xmm5
  2dd30b:	66 0f 6f 3d ed 83 d6 	movdqa -0x297c13(%rip),%xmm7        # 45700 <GCC_except_table2456+0x165c>
  2dd312:	ff 
  2dd313:	66 0f ef f6          	pxor   %xmm6,%xmm6
  2dd317:	66 0f ef e4          	pxor   %xmm4,%xmm4
  2dd31b:	66 0f ef db          	pxor   %xmm3,%xmm3
  2dd31f:	66 0f ef c0          	pxor   %xmm0,%xmm0
  2dd323:	66 66 66 66 2e 0f 1f 	data16 data16 data16 cs nopw 0x0(%rax,%rax,1)
  2dd32a:	84 00 00 00 00 00 
  2dd330:	0f b7 34 11          	movzwl (%rcx,%rdx,1),%esi
  2dd334:	66 44 0f 6e ce       	movd   %esi,%xmm9
  2dd339:	0f b7 74 11 02       	movzwl 0x2(%rcx,%rdx,1),%esi
  2dd33e:	66 44 0f 6e c6       	movd   %esi,%xmm8
  2dd343:	66 44 0f 60 ca       	punpcklbw %xmm2,%xmm9
  2dd348:	66 44 0f 60 c2       	punpcklbw %xmm2,%xmm8
  2dd34d:	66 45 0f 70 d1 00    	pshufd $0x0,%xmm9,%xmm10
  2dd353:	f3 45 0f 70 d2 55    	pshufhw $0x55,%xmm10,%xmm10
  2dd359:	66 44 0f 6f dd       	movdqa %xmm5,%xmm11
  2dd35e:	66 45 0f d9 da       	psubusw %xmm10,%xmm11
  2dd363:	66 44 0f 75 da       	pcmpeqw %xmm2,%xmm11
  2dd368:	66 44 0f db df       	pand   %xmm7,%xmm11
  2dd36d:	66 41 0f d4 db       	paddq  %xmm11,%xmm3
  2dd372:	66 45 0f 70 d0 00    	pshufd $0x0,%xmm8,%xmm10
  2dd378:	f3 45 0f 70 d2 55    	pshufhw $0x55,%xmm10,%xmm10
  2dd37e:	66 44 0f 6f dd       	movdqa %xmm5,%xmm11
  2dd383:	66 45 0f d9 da       	psubusw %xmm10,%xmm11
  2dd388:	66 44 0f 75 da       	pcmpeqw %xmm2,%xmm11
  2dd38d:	66 44 0f db df       	pand   %xmm7,%xmm11
  2dd392:	66 41 0f d4 c3       	paddq  %xmm11,%xmm0
  2dd397:	66 44 0f 6f d1       	movdqa %xmm1,%xmm10
  2dd39c:	66 45 0f d9 d1       	psubusw %xmm9,%xmm10
  2dd3a1:	66 45 0f 70 ca 00    	pshufd $0x0,%xmm10,%xmm9
  2dd3a7:	f3 45 0f 70 c9 55    	pshufhw $0x55,%xmm9,%xmm9
  2dd3ad:	66 44 0f 75 ca       	pcmpeqw %xmm2,%xmm9
  2dd3b2:	66 44 0f db cf       	pand   %xmm7,%xmm9
  2dd3b7:	66 41 0f d4 f1       	paddq  %xmm9,%xmm6
  2dd3bc:	66 44 0f 6f c9       	movdqa %xmm1,%xmm9
  2dd3c1:	66 45 0f d9 c8       	psubusw %xmm8,%xmm9
  2dd3c6:	66 45 0f 70 c1 00    	pshufd $0x0,%xmm9,%xmm8
  2dd3cc:	f3 45 0f 70 c0 55    	pshufhw $0x55,%xmm8,%xmm8
  2dd3d2:	66 44 0f 75 c2       	pcmpeqw %xmm2,%xmm8
  2dd3d7:	66 44 0f db c7       	pand   %xmm7,%xmm8
  2dd3dc:	66 41 0f d4 e0       	paddq  %xmm8,%xmm4
  2dd3e1:	48 83 c2 04          	add    $0x4,%rdx
  2dd3e5:	49 39 d1             	cmp    %rdx,%r9
  2dd3e8:	0f 85 42 ff ff ff    	jne    2dd330 <rust_fx::commands::collect_stats::{closure#1}+0x4e0>
  2dd3ee:	66 0f d4 e6          	paddq  %xmm6,%xmm4
  2dd3f2:	66 0f 70 cc ee       	pshufd $0xee,%xmm4,%xmm1
  2dd3f7:	66 0f d4 cc          	paddq  %xmm4,%xmm1
  2dd3fb:	66 48 0f 7e ca       	movq   %xmm1,%rdx
  2dd400:	66 0f d4 c3          	paddq  %xmm3,%xmm0
  2dd404:	66 0f 70 c8 ee       	pshufd $0xee,%xmm0,%xmm1
  2dd409:	66 0f d4 c8          	paddq  %xmm0,%xmm1
  2dd40d:	66 48 0f 7e ce       	movq   %xmm1,%rsi
  2dd412:	4c 39 c8             	cmp    %r9,%rax
  2dd415:	75 09                	jne    2dd420 <rust_fx::commands::collect_stats::{closure#1}+0x5d0>
  2dd417:	89 c7                	mov    %eax,%edi
  2dd419:	83 e7 03             	and    $0x3,%edi
  2dd41c:	eb 33                	jmp    2dd451 <rust_fx::commands::collect_stats::{closure#1}+0x601>
  2dd41e:	66 90                	xchg   %ax,%ax
  2dd420:	46 0f b6 14 09       	movzbl (%rcx,%r9,1),%r10d
  2dd425:	66 41 39 fa          	cmp    %di,%r10w
  2dd429:	48 83 de ff          	sbb    $0xffffffffffffffff,%rsi
  2dd42d:	66 45 39 c2          	cmp    %r8w,%r10w
  2dd431:	48 83 da ff          	sbb    $0xffffffffffffffff,%rdx
  2dd435:	49 ff c1             	inc    %r9
  2dd438:	4c 39 c8             	cmp    %r9,%rax
  2dd43b:	75 e3                	jne    2dd420 <rust_fx::commands::collect_stats::{closure#1}+0x5d0>
  2dd43d:	89 c7                	mov    %eax,%edi
  2dd43f:	83 e7 03             	and    $0x3,%edi
  2dd442:	48 83 f8 04          	cmp    $0x4,%rax
  2dd446:	73 09                	jae    2dd451 <rust_fx::commands::collect_stats::{closure#1}+0x601>
  2dd448:	66 0f ef c0          	pxor   %xmm0,%xmm0
  2dd44c:	45 31 c0             	xor    %r8d,%r8d
  2dd44f:	eb 50                	jmp    2dd4a1 <rust_fx::commands::collect_stats::{closure#1}+0x651>
  2dd451:	49 89 c1             	mov    %rax,%r9
  2dd454:	49 83 e1 fc          	and    $0xfffffffffffffffc,%r9
  2dd458:	66 0f ef c0          	pxor   %xmm0,%xmm0
  2dd45c:	45 31 c0             	xor    %r8d,%r8d
  2dd45f:	90                   	nop
  2dd460:	46 0f b6 14 01       	movzbl (%rcx,%r8,1),%r10d
  2dd465:	f2 43 0f 58 44 d6 08 	addsd  0x8(%r14,%r10,8),%xmm0
  2dd46c:	46 0f b6 54 01 01    	movzbl 0x1(%rcx,%r8,1),%r10d
  2dd472:	f2 43 0f 58 44 d6 08 	addsd  0x8(%r14,%r10,8),%xmm0
  2dd479:	46 0f b6 54 01 02    	movzbl 0x2(%rcx,%r8,1),%r10d
  2dd47f:	f2 43 0f 58 44 d6 08 	addsd  0x8(%r14,%r10,8),%xmm0
  2dd486:	46 0f b6 54 01 03    	movzbl 0x3(%rcx,%r8,1),%r10d
  2dd48c:	f2 43 0f 58 44 d6 08 	addsd  0x8(%r14,%r10,8),%xmm0
  2dd493:	49 83 c0 04          	add    $0x4,%r8
  2dd497:	4d 39 c1             	cmp    %r8,%r9
  2dd49a:	75 c4                	jne    2dd460 <rust_fx::commands::collect_stats::{closure#1}+0x610>
  2dd49c:	48 85 ff             	test   %rdi,%rdi
  2dd49f:	74 23                	je     2dd4c4 <rust_fx::commands::collect_stats::{closure#1}+0x674>
  2dd4a1:	4c 01 c1             	add    %r8,%rcx
  2dd4a4:	45 31 c0             	xor    %r8d,%r8d
  2dd4a7:	66 0f 1f 84 00 00 00 	nopw   0x0(%rax,%rax,1)
  2dd4ae:	00 00 
  2dd4b0:	46 0f b6 0c 01       	movzbl (%rcx,%r8,1),%r9d
  2dd4b5:	f2 43 0f 58 44 ce 08 	addsd  0x8(%r14,%r9,8),%xmm0
  2dd4bc:	49 ff c0             	inc    %r8
  2dd4bf:	4c 39 c7             	cmp    %r8,%rdi
  2dd4c2:	75 ec                	jne    2dd4b0 <rust_fx::commands::collect_stats::{closure#1}+0x660>
  2dd4c4:	49 01 43 58          	add    %rax,0x58(%r11)
  2dd4c8:	49 01 73 60          	add    %rsi,0x60(%r11)
  2dd4cc:	49 01 53 68          	add    %rdx,0x68(%r11)
  2dd4d0:	f2 41 0f 58 43 70    	addsd  0x70(%r11),%xmm0
  2dd4d6:	f2 41 0f 11 43 70    	movsd  %xmm0,0x70(%r11)
  2dd4dc:	48 81 c4 98 00 00 00 	add    $0x98,%rsp
  2dd4e3:	5b                   	pop    %rbx
  2dd4e4:	41 5c                	pop    %r12
  2dd4e6:	41 5d                	pop    %r13
  2dd4e8:	41 5e                	pop    %r14
  2dd4ea:	41 5f                	pop    %r15
  2dd4ec:	5d                   	pop    %rbp
  2dd4ed:	c3                   	ret
  2dd4ee:	66 41 83 ff 0b       	cmp    $0xb,%r15w
  2dd4f3:	0f 83 82 00 00 00    	jae    2dd57b <rust_fx::commands::collect_stats::{closure#1}+0x72b>
  2dd4f9:	4d 39 ef             	cmp    %r13,%r15
  2dd4fc:	76 5f                	jbe    2dd55d <rust_fx::commands::collect_stats::{closure#1}+0x70d>
  2dd4fe:	4b 8d 04 ec          	lea    (%r12,%r13,8),%rax
  2dd502:	4b 8d 3c ec          	lea    (%r12,%r13,8),%rdi
  2dd506:	48 83 c7 08          	add    $0x8,%rdi
  2dd50a:	4c 89 74 24 30       	mov    %r14,0x30(%rsp)
  2dd50f:	45 89 fe             	mov    %r15d,%r14d
  2dd512:	45 29 ee             	sub    %r13d,%r14d
  2dd515:	41 c1 e6 03          	shl    $0x3,%r14d
  2dd519:	48 89 74 24 20       	mov    %rsi,0x20(%rsp)
  2dd51e:	48 89 c6             	mov    %rax,%rsi
  2dd521:	4c 89 f2             	mov    %r14,%rdx
  2dd524:	4d 89 dc             	mov    %r11,%r12
  2dd527:	4c 89 54 24 10       	mov    %r10,0x10(%rsp)
  2dd52c:	ff 15 46 74 17 00    	call   *0x177446(%rip)        # 454978 <memmove@GLIBC_2.2.5>
  2dd532:	4a 8d 34 eb          	lea    (%rbx,%r13,8),%rsi
  2dd536:	48 83 c6 60          	add    $0x60,%rsi
  2dd53a:	4a 8d 3c eb          	lea    (%rbx,%r13,8),%rdi
  2dd53e:	48 83 c7 68          	add    $0x68,%rdi
  2dd542:	4c 89 f2             	mov    %r14,%rdx
  2dd545:	4c 8b 74 24 30       	mov    0x30(%rsp),%r14
  2dd54a:	ff 15 28 74 17 00    	call   *0x177428(%rip)        # 454978 <memmove@GLIBC_2.2.5>
  2dd550:	4c 8b 54 24 10       	mov    0x10(%rsp),%r10
  2dd555:	4d 89 e3             	mov    %r12,%r11
  2dd558:	48 8b 74 24 20       	mov    0x20(%rsp),%rsi
  2dd55d:	4a 89 6c eb 08       	mov    %rbp,0x8(%rbx,%r13,8)
  2dd562:	41 8d 47 01          	lea    0x1(%r15),%eax
  2dd566:	4a c7 44 eb 60 00 00 	movq   $0x0,0x60(%rbx,%r13,8)
  2dd56d:	00 00 
  2dd56f:	66 89 83 ba 00 00 00 	mov    %ax,0xba(%rbx)
  2dd576:	e9 8a fc ff ff       	jmp    2dd205 <rust_fx::commands::collect_stats::{closure#1}+0x3b5>
  2dd57b:	49 83 fd 05          	cmp    $0x5,%r13
  2dd57f:	48 89 74 24 20       	mov    %rsi,0x20(%rsp)
  2dd584:	4c 89 5c 24 50       	mov    %r11,0x50(%rsp)
  2dd589:	4c 89 74 24 30       	mov    %r14,0x30(%rsp)
  2dd58e:	4c 89 54 24 10       	mov    %r10,0x10(%rsp)
  2dd593:	48 89 bc 24 88 00 00 	mov    %rdi,0x88(%rsp)
  2dd59a:	00 
  2dd59b:	4c 89 84 24 80 00 00 	mov    %r8,0x80(%rsp)
  2dd5a2:	00 
  2dd5a3:	73 0f                	jae    2dd5b4 <rust_fx::commands::collect_stats::{closure#1}+0x764>
  2dd5a5:	41 bf 04 00 00 00    	mov    $0x4,%r15d
  2dd5ab:	c7 04 24 00 00 00 00 	movl   $0x0,(%rsp)
  2dd5b2:	eb 33                	jmp    2dd5e7 <rust_fx::commands::collect_stats::{closure#1}+0x797>
  2dd5b4:	74 16                	je     2dd5cc <rust_fx::commands::collect_stats::{closure#1}+0x77c>
  2dd5b6:	49 83 fd 06          	cmp    $0x6,%r13
  2dd5ba:	75 1c                	jne    2dd5d8 <rust_fx::commands::collect_stats::{closure#1}+0x788>
  2dd5bc:	41 bf 05 00 00 00    	mov    $0x5,%r15d
  2dd5c2:	b0 01                	mov    $0x1,%al
  2dd5c4:	89 04 24             	mov    %eax,(%rsp)
  2dd5c7:	45 31 ed             	xor    %r13d,%r13d
  2dd5ca:	eb 1b                	jmp    2dd5e7 <rust_fx::commands::collect_stats::{closure#1}+0x797>
  2dd5cc:	c7 04 24 00 00 00 00 	movl   $0x0,(%rsp)
  2dd5d3:	4d 89 ef             	mov    %r13,%r15
  2dd5d6:	eb 0f                	jmp    2dd5e7 <rust_fx::commands::collect_stats::{closure#1}+0x797>
  2dd5d8:	49 83 c5 f9          	add    $0xfffffffffffffff9,%r13
  2dd5dc:	41 bf 06 00 00 00    	mov    $0x6,%r15d
  2dd5e2:	b0 01                	mov    $0x1,%al
  2dd5e4:	89 04 24             	mov    %eax,(%rsp)
  2dd5e7:	bf c0 00 00 00       	mov    $0xc0,%edi
  2dd5ec:	ff 15 5e 73 17 00    	call   *0x17735e(%rip)        # 454950 <malloc@GLIBC_2.2.5>
  2dd5f2:	48 85 c0             	test   %rax,%rax
  2dd5f5:	0f 84 89 07 00 00    	je     2ddd84 <rust_fx::commands::collect_stats::{closure#1}+0xf34>
  2dd5fb:	49 89 c6             	mov    %rax,%r14
  2dd5fe:	48                   	rex.W
  2dd5ff:	c7                   	.byte 0xc7
