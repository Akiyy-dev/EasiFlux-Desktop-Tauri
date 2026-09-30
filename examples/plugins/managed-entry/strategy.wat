(module
  (memory (export "memory") 1 16)
  ;; NUL-terminated tokens and fragments in bounded MVP data segments.
  (data (i32.const 256)
    ;; 256
    "{\"state\":{\"phase\":\"done\"},\"action\":{\"kind\":\"stop\"},\"message\":\"Stopped; acknowledgement is not active protection.\"}\00"
    ;; 371
    "{\"state\":\00"
    ;; 381
    ",\"action\":{\"kind\":\"none\"},\"message\":\"Waiting; no mutation is retried.\"}\00"
    ;; 453
    "\"symbol\"\00"
    ;; 462
    "\"qty\"\00"
    ;; 468
    "\"entryPrice\"\00"
    ;; 481
    "\"amendPrice\"\00"
    ;; 494
    "\"takeProfit\"\00"
    ;; 507
    "\"stopLoss\"\00"
    ;; 518
    "\"state\"\00"
    ;; 526
    "\"sequence\"\00"
    ;; 537
    "\"lastReceipt\"\00"
    ;; 551
    "{\"state\":{\"phase\":\"placed\",\"sequence\":\00"
    ;; 590
    "},\"action\":{\"kind\":\"placeProtectedOrder\",\"order\":{\"symbol\":\00"
    ;; 650
    ",\"side\":\"Buy\",\"orderType\":\"Limit\",\"qty\":\00"
    ;; 691
    ",\"price\":\00"
    ;; 701
    ",\"timeInForce\":\"GTC\",\"positionIdx\":1,\"reduceOnly\":false},\"protection\":{\"takeProfit\":\00"
    ;; 786
    ",\"stopLoss\":\00"
    ;; 799
    ",\"triggerBy\":\"LastPrice\"}},\"message\":\"Requesting one protected entry; acknowledgement is not activation.\"}\00"
    ;; 906
    "\"phase\"\00"
    ;; 914
    "\"done\"\00"
    ;; 921
    "\"placed\"\00"
    ;; 930
    "\"kind\"\00"
    ;; 937
    "\"placeProtectedOrder\"\00"
    ;; 959
    "\"status\"\00"
    ;; 968
    "\"accepted\"\00"
    ;; 979
    "\"errorCode\"\00"
    ;; 991
    "null\00"
    ;; 996
    "\"orderId\"\00"
    ;; 1006
    "\"submissionId\"\00"
    ;; 1021
    "{\"state\":{\"phase\":\"awaitingOrder\",\"sequence\":\00"
    ;; 1067
    ",\"orderId\":\00"
    ;; 1079
    ",\"submissionId\":\00"
    ;; 1096
    "},\"action\":{\"kind\":\"none\"},\"message\":\"Acknowledged; waiting for exact owned visible order.\"}\00"
    ;; 1189
    "\"amendRequested\"\00"
    ;; 1206
    "\"amendOrder\"\00"
    ;; 1219
    "\"awaitingOrder\"\00"
    ;; 1235
    "\"snapshot\"\00"
    ;; 1246
    "\"orders\"\00"
    ;; 1255
    "\"items\"\00"
    ;; 1263
    "\"orderLinkId\"\00"
    ;; 1277
    "\"side\"\00"
    ;; 1284
    "\"Buy\"\00"
    ;; 1290
    "\"orderType\"\00"
    ;; 1302
    "\"Limit\"\00"
    ;; 1310
    "\"New\"\00"
    ;; 1316
    "\"PartiallyFilled\"\00"
    ;; 1334
    "\"filledQty\"\00"
    ;; 1346
    "\"price\"\00"
    ;; 1354
    "{\"state\":{\"phase\":\"amendRequested\",\"sequence\":\00"
    ;; 1401
    "},\"action\":{\"kind\":\"amendOrder\",\"order\":{\"symbol\":\00"
    ;; 1452
    ",\"qty\":\00"
    ;; 1460
    "}},\"message\":\"Requesting one amendment; waiting for acknowledgement.\"}\00")

  ;; 8192 = event key; 8200 = start; 8208 = initial sequence.
  (data (i32.const 8192) "\"event\"\00\"start\"\00\"1\"\00")

  ;; Scratch words 16000/16004 hold the parser limit/output cursor, never authority.
  ;; Output begins at 24576. No mutable globals, imports or bulk memory.
  (func (export "alloc") (param $n i32) (result i32)
    local.get $n i32.const 65535 i32.add i32.const 16 i32.shr_u memory.grow i32.const 65536 i32.mul)
  (func $byte (param $p i32) (result i32)
    local.get $p i32.const 16000 i32.load i32.ge_u if unreachable end local.get $p i32.load8_u)
  (func $ws (param $p i32) (result i32)
    (loop $next local.get $p call $byte i32.const 32 i32.le_u
      if local.get $p i32.const 1 i32.add local.set $p br $next end) local.get $p)
  ;; Structural bounded JSON walker. Copied values get stricter allowlists below.
  ;; Keys are looked up within their own object, not with substring matching.
  (func $end (param $p i32) (param $depth i32) (result i32)
    (local $c i32) (local $close i32)
    local.get $depth i32.const 32 i32.gt_u if unreachable end
    local.get $p call $ws local.set $p local.get $p call $byte local.set $c
    local.get $c i32.const 34 i32.eq if
      local.get $p i32.const 1 i32.add local.set $p
      (loop $string
        local.get $p call $byte local.set $c
        local.get $c i32.const 34 i32.eq if local.get $p i32.const 1 i32.add return end
        local.get $c i32.const 32 i32.lt_u if unreachable end
        local.get $c i32.const 92 i32.eq if
          local.get $p i32.const 1 i32.add local.set $p local.get $p call $byte drop end
        local.get $p i32.const 1 i32.add local.set $p br $string)
    end
    local.get $c i32.const 123 i32.eq local.get $c i32.const 91 i32.eq i32.or if
      local.get $c i32.const 2 i32.add local.set $close
      local.get $p i32.const 1 i32.add call $ws local.set $p
      local.get $p call $byte local.get $close i32.eq if local.get $p i32.const 1 i32.add return end
      (loop $member
        local.get $close i32.const 125 i32.eq if
          local.get $p call $byte i32.const 34 i32.ne if unreachable end
          local.get $p local.get $depth i32.const 1 i32.add call $end call $ws local.set $p
          local.get $p call $byte i32.const 58 i32.ne if unreachable end
          local.get $p i32.const 1 i32.add call $ws local.set $p
        end
        local.get $p local.get $depth i32.const 1 i32.add call $end call $ws local.set $p
        local.get $p call $byte local.get $close i32.eq if local.get $p i32.const 1 i32.add return end
        local.get $p call $byte i32.const 44 i32.ne if unreachable end
        local.get $p i32.const 1 i32.add call $ws local.set $p br $member)
    end
    ;; Host primitive values are not copied into actions.
    (loop $primitive
      local.get $p call $byte local.set $c
      local.get $c i32.const 44 i32.eq local.get $c i32.const 125 i32.eq i32.or
      local.get $c i32.const 93 i32.eq i32.or local.get $c i32.const 32 i32.le_u i32.or
      if local.get $p return end
      local.get $p i32.const 1 i32.add local.set $p br $primitive)
    unreachable)
  (func $length (param $p i32) (result i32) (local $n i32)
    (loop $next local.get $p local.get $n i32.add i32.load8_u i32.eqz if local.get $n return end
      local.get $n i32.const 1 i32.add local.set $n br $next) unreachable)
  (func $equal (param $a i32) (param $b i32) (param $n i32) (result i32) (local $i i32)
    (block $done (loop $next local.get $i local.get $n i32.ge_u br_if $done
      local.get $a local.get $i i32.add i32.load8_u local.get $b local.get $i i32.add i32.load8_u
      i32.ne if i32.const 0 return end
      local.get $i i32.const 1 i32.add local.set $i br $next)) i32.const 1)
  (func $eq (param $a i32) (param $constant i32) (result i32) (local $n i32)
    local.get $a i32.eqz if i32.const 0 return end
    local.get $constant call $length local.set $n
    local.get $a i32.const 0 call $end local.get $a i32.sub local.get $n i32.ne if i32.const 0 return end
    local.get $a local.get $constant local.get $n call $equal)
  (func $same (param $a i32) (param $b i32) (result i32) (local $n i32)
    local.get $a i32.eqz local.get $b i32.eqz i32.or if i32.const 0 return end
    local.get $a i32.const 0 call $end local.get $a i32.sub local.set $n
    local.get $b i32.const 0 call $end local.get $b i32.sub local.get $n i32.ne if i32.const 0 return end
    local.get $a local.get $b local.get $n call $equal)
  (func $field (param $obj i32) (param $key i32) (result i32)
    (local $p i32) (local $match i32) (local $found i32)
    local.get $obj i32.eqz if i32.const 0 return end
    local.get $obj call $byte i32.const 123 i32.ne if i32.const 0 return end
    local.get $obj i32.const 1 i32.add call $ws local.set $p
    (block $done (loop $next
      local.get $p call $byte i32.const 125 i32.eq br_if $done
      local.get $p local.get $key call $eq local.set $match
      local.get $p i32.const 0 call $end call $ws i32.const 1 i32.add call $ws local.set $p
      local.get $match if local.get $found if unreachable end local.get $p local.set $found end
      local.get $p i32.const 0 call $end call $ws local.set $p
      local.get $p call $byte i32.const 44 i32.eq if local.get $p i32.const 1 i32.add call $ws local.set $p end
      br $next)) local.get $found)
  (func $count (param $obj i32) (result i32) (local $p i32) (local $n i32)
    local.get $obj call $byte i32.const 123 i32.ne if unreachable end
    local.get $obj i32.const 1 i32.add call $ws local.set $p
    (block $done (loop $next
      local.get $p call $byte i32.const 125 i32.eq br_if $done
      local.get $p i32.const 0 call $end call $ws i32.const 1 i32.add call $ws
      i32.const 0 call $end call $ws local.set $p
      local.get $n i32.const 1 i32.add local.set $n
      local.get $p call $byte i32.const 44 i32.eq if local.get $p i32.const 1 i32.add call $ws local.set $p end
      br $next)) local.get $n)
  ;; mode 0: ID [A-Za-z0-9_-], 128 bytes; 1: positive decimal, 64 bytes;
  ;; 2: nonnegative decimal; 3: symbol [A-Z0-9], 32 bytes. No escaped output.
  (func $text (param $p i32) (param $mode i32)
    (local $i i32) (local $c i32) (local $n i32) (local $dot i32) (local $positive i32)
    local.get $p i32.eqz if unreachable end local.get $p call $byte i32.const 34 i32.ne if unreachable end
    local.get $p i32.const 0 call $end local.get $p i32.sub i32.const 2 i32.sub local.set $n
    local.get $n i32.eqz local.get $n i32.const 128 i32.gt_u i32.or if unreachable end
    local.get $mode i32.const 1 i32.eq local.get $mode i32.const 2 i32.eq i32.or if
      local.get $n i32.const 64 i32.gt_u if unreachable end
      local.get $n i32.const 1 i32.gt_u local.get $p i32.const 1 i32.add i32.load8_u i32.const 48 i32.eq i32.and
      local.get $p i32.const 2 i32.add i32.load8_u i32.const 46 i32.ne i32.and if unreachable end
    end
    (block $done (loop $next
      local.get $i local.get $n i32.ge_u br_if $done
      local.get $p local.get $i i32.add i32.const 1 i32.add i32.load8_u local.set $c
      local.get $mode i32.const 1 i32.eq local.get $mode i32.const 2 i32.eq i32.or if
        local.get $c i32.const 48 i32.ge_u local.get $c i32.const 57 i32.le_u i32.and
        if local.get $positive local.get $c i32.const 48 i32.ne i32.or local.set $positive
        else
          local.get $c i32.const 46 i32.ne local.get $dot i32.or local.get $i i32.eqz i32.or
          local.get $i i32.const 1 i32.add local.get $n i32.eq i32.or if unreachable end i32.const 1 local.set $dot
        end
      else
        local.get $c i32.const 48 i32.ge_u local.get $c i32.const 57 i32.le_u i32.and
        local.get $c i32.const 65 i32.ge_u local.get $c i32.const 90 i32.le_u i32.and i32.or
        local.get $mode i32.eqz
        local.get $c i32.const 97 i32.ge_u local.get $c i32.const 122 i32.le_u i32.and
        local.get $c i32.const 45 i32.eq i32.or local.get $c i32.const 95 i32.eq i32.or i32.and i32.or
        i32.eqz if unreachable end
      end
      local.get $i i32.const 1 i32.add local.set $i br $next))
    local.get $mode i32.const 1 i32.eq local.get $positive i32.eqz i32.and if unreachable end
    local.get $mode i32.const 3 i32.eq local.get $n i32.const 32 i32.gt_u i32.and if unreachable end)
  ;; Exact decimal comparison (-1/0/1), no floats or integer overflow.
  (func $integer (param $p i32) (result i32) (local $n i32) (local $c i32)
    (loop $next local.get $p local.get $n i32.add i32.load8_u local.set $c
      local.get $c i32.const 46 i32.eq local.get $c i32.const 34 i32.eq i32.or if local.get $n return end
      local.get $n i32.const 1 i32.add local.set $n br $next) unreachable)
  (func $decimal (param $a i32) (param $b i32) (result i32)
    (local $ai i32) (local $bi i32) (local $i i32) (local $ac i32) (local $bc i32)
    local.get $a i32.const 1 i32.add local.set $a local.get $b i32.const 1 i32.add local.set $b
    local.get $a call $integer local.set $ai local.get $b call $integer local.set $bi
    local.get $ai local.get $bi i32.gt_u if i32.const 1 return end
    local.get $ai local.get $bi i32.lt_u if i32.const -1 return end
    (loop $digits
      local.get $a local.get $i i32.add i32.load8_u local.set $ac
      local.get $b local.get $i i32.add i32.load8_u local.set $bc
      local.get $ac i32.const 34 i32.eq local.get $bc i32.const 34 i32.eq i32.and if i32.const 0 return end
      local.get $ac i32.const 34 i32.eq if i32.const 48 local.set $ac local.get $a i32.const 1 i32.sub local.set $a end
      local.get $bc i32.const 34 i32.eq if i32.const 48 local.set $bc local.get $b i32.const 1 i32.sub local.set $b end
      local.get $i local.get $ai i32.eq if
        local.get $ac i32.const 46 i32.eq if i32.const 48 local.set $ac end
        local.get $bc i32.const 46 i32.eq if i32.const 48 local.set $bc end
      end
      local.get $ac local.get $bc i32.gt_u if i32.const 1 return end
      local.get $ac local.get $bc i32.lt_u if i32.const -1 return end
      local.get $i i32.const 1 i32.add local.set $i br $digits) unreachable)
  (func $copy (param $src i32) (param $n i32) (local $i i32) (local $dst i32)
    i32.const 16004 i32.load local.set $dst
    (block $done (loop $next local.get $i local.get $n i32.ge_u br_if $done
      local.get $dst local.get $i i32.add local.get $src local.get $i i32.add i32.load8_u i32.store8
      local.get $i i32.const 1 i32.add local.set $i br $next))
    i32.const 16004 local.get $dst local.get $n i32.add i32.store)
  (func $literal (param $p i32) local.get $p local.get $p call $length call $copy)
  (func $token (param $p i32) local.get $p local.get $p i32.const 0 call $end local.get $p i32.sub call $copy)
  (func $finish (result i64)
    i64.const 105553116266496 i32.const 16004 i32.load i32.const 24576 i32.sub i64.extend_i32_u i64.or)
  (func $stop (result i64)
    i32.const 256 call $literal call $finish)
  (func $wait (param $state i32) (result i64)
    i32.const 371 call $literal local.get $state call $token
    i32.const 381 call $literal call $finish)
  (func (export "run") (param $ctx i32) (param $cn i32) (param $input i32) (param $in i32) (result i64)
    (local $symbol i32) (local $qty i32) (local $entry i32) (local $amend i32) (local $tp i32) (local $sl i32)
    (local $state i32) (local $phase i32) (local $seq i32) (local $receipt i32) (local $id i32) (local $link i32)
    (local $snapshot i32) (local $orders i32) (local $row i32) (local $matches i32) (local $candidate i32)
    (local $value i32) (local $state_seq i32)
    i32.const 16004 i32.const 24576 i32.store
    local.get $in i32.eqz local.get $in i32.const 4096 i32.gt_u i32.or
    local.get $cn i32.const 65536 i32.gt_u i32.or if unreachable end
    i32.const 16000 local.get $ctx local.get $cn i32.add i32.store
    local.get $ctx i32.const 0 call $end local.get $ctx local.get $cn i32.add i32.ne if unreachable end
    i32.const 16000 local.get $input local.get $in i32.add i32.store
    local.get $input i32.const 0 call $end local.get $input local.get $in i32.add i32.ne if unreachable end
    local.get $input call $count i32.const 6 i32.ne if unreachable end
    local.get $input i32.const 453 call $field local.set $symbol local.get $symbol i32.const 3 call $text
    local.get $input i32.const 462 call $field local.set $qty local.get $qty i32.const 1 call $text
    local.get $input i32.const 468 call $field local.set $entry local.get $entry i32.const 1 call $text
    local.get $input i32.const 481 call $field local.set $amend local.get $amend i32.const 1 call $text
    local.get $input i32.const 494 call $field local.set $tp local.get $tp i32.const 1 call $text
    local.get $input i32.const 507 call $field local.set $sl local.get $sl i32.const 1 call $text
    local.get $entry local.get $amend call $decimal i32.eqz if unreachable end
    local.get $tp local.get $entry call $decimal i32.const 1 i32.ne
    local.get $tp local.get $amend call $decimal i32.const 1 i32.ne i32.or
    local.get $entry local.get $sl call $decimal i32.const 1 i32.ne i32.or
    local.get $amend local.get $sl call $decimal i32.const 1 i32.ne i32.or if unreachable end
    local.get $ctx i32.const 518 call $field local.set $state
    local.get $state i32.const 0 call $end local.get $state i32.sub i32.const 4096 i32.gt_u if unreachable end
    local.get $ctx i32.const 526 call $field local.set $seq local.get $seq i32.const 1 call $text
    local.get $ctx i32.const 537 call $field local.set $receipt
    ;; Persist submission phase before any future callback can request another action.
    local.get $state call $count i32.eqz if
      local.get $seq i32.const 8208 call $eq
      local.get $ctx i32.const 8192 call $field i32.const 8200 call $eq i32.and
      local.get $receipt i32.const 991 call $eq i32.and i32.eqz
      if call $stop return end
      i32.const 551 call $literal local.get $seq call $token
      i32.const 590 call $literal local.get $symbol call $token
      i32.const 650 call $literal local.get $qty call $token
      i32.const 691 call $literal local.get $entry call $token
      i32.const 701 call $literal local.get $tp call $token
      i32.const 786 call $literal local.get $sl call $token
      i32.const 799 call $literal
      call $finish return
    end
    local.get $state i32.const 906 call $field local.set $phase
    local.get $phase i32.const 914 call $eq if
      local.get $state call $count i32.const 1 i32.ne if unreachable end call $stop return end
    local.get $state i32.const 526 call $field local.set $state_seq local.get $state_seq i32.const 1 call $text
    local.get $phase i32.const 921 call $eq if
      local.get $state call $count i32.const 2 i32.ne if unreachable end
      local.get $receipt i32.const 930 call $field i32.const 937 call $eq
      local.get $receipt i32.const 526 call $field local.get $state_seq call $same i32.and i32.eqz
      if local.get $state call $wait return end
      local.get $receipt i32.const 959 call $field i32.const 968 call $eq i32.eqz if call $stop return end
      local.get $receipt i32.const 979 call $field i32.const 991 call $eq i32.eqz if unreachable end
      local.get $receipt i32.const 996 call $field local.set $id local.get $id i32.const 0 call $text
      local.get $receipt i32.const 1006 call $field local.set $link local.get $link i32.const 0 call $text
      i32.const 1021 call $literal local.get $state_seq call $token
      i32.const 1067 call $literal local.get $id call $token i32.const 1079 call $literal local.get $link call $token
      i32.const 1096 call $literal call $finish return
    end
    local.get $state call $count i32.const 4 i32.ne if unreachable end
    local.get $state i32.const 996 call $field local.set $id local.get $id i32.const 0 call $text
    local.get $state i32.const 1006 call $field local.set $link local.get $link i32.const 0 call $text
    local.get $phase i32.const 1189 call $eq if
      local.get $receipt i32.const 930 call $field i32.const 1206 call $eq
      local.get $receipt i32.const 526 call $field local.get $state_seq call $same i32.and
      local.get $receipt i32.const 996 call $field local.get $id call $same i32.and i32.eqz
      if local.get $state call $wait return end
      local.get $receipt i32.const 959 call $field i32.const 968 call $eq if
        local.get $receipt i32.const 979 call $field i32.const 991 call $eq
        local.get $receipt i32.const 1006 call $field i32.const 991 call $eq i32.and i32.eqz if unreachable end
      end call $stop return
    end
    local.get $phase i32.const 1219 call $eq i32.eqz if unreachable end
    local.get $ctx i32.const 1235 call $field local.set $snapshot
    local.get $snapshot i32.const 1246 call $field i32.const 1255 call $field local.set $orders
    local.get $orders i32.eqz if local.get $state call $wait return end
    local.get $orders call $byte i32.const 91 i32.ne if local.get $state call $wait return end
    local.get $orders i32.const 1 i32.add call $ws local.set $row
    (block $scanned (loop $next
      local.get $row call $byte i32.const 93 i32.eq br_if $scanned
      local.get $row i32.const 996 call $field local.get $id call $same if
        local.get $matches i32.const 1 i32.add local.set $matches local.get $row local.set $candidate end
      local.get $row i32.const 0 call $end call $ws local.set $row
      local.get $row call $byte i32.const 44 i32.eq if local.get $row i32.const 1 i32.add call $ws local.set $row end br $next))
    local.get $matches i32.const 1 i32.ne if local.get $state call $wait return end
    local.get $candidate i32.const 1263 call $field local.get $link call $same
    local.get $candidate i32.const 453 call $field local.get $symbol call $same i32.and
    local.get $candidate i32.const 1277 call $field i32.const 1284 call $eq i32.and
    local.get $candidate i32.const 1290 call $field i32.const 1302 call $eq i32.and
    local.get $candidate i32.const 959 call $field i32.const 1310 call $eq
    local.get $candidate i32.const 959 call $field i32.const 1316 call $eq i32.or i32.and i32.eqz
    if local.get $state call $wait return end
    local.get $candidate i32.const 462 call $field local.set $value local.get $value i32.const 1 call $text
    local.get $value local.get $qty call $decimal i32.const 0 i32.lt_s if local.get $state call $wait return end
    local.get $candidate i32.const 1334 call $field local.set $value local.get $value i32.const 2 call $text
    local.get $qty local.get $value call $decimal i32.const 1 i32.ne if local.get $state call $wait return end
    local.get $candidate i32.const 1346 call $field local.set $value local.get $value i32.const 1 call $text
    local.get $amend local.get $value call $decimal i32.eqz if local.get $state call $wait return end
    i32.const 1354 call $literal local.get $seq call $token
    i32.const 1067 call $literal local.get $id call $token i32.const 1079 call $literal local.get $link call $token
    i32.const 1401 call $literal local.get $symbol call $token
    i32.const 1067 call $literal local.get $id call $token i32.const 691 call $literal local.get $amend call $token i32.const 1452 call $literal local.get $qty call $token
    i32.const 1460 call $literal call $finish)
)
