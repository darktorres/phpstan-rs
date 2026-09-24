<?php
if (is_writable('a.php')) { include 'a.php'; }
if (is_writeable('b.php')) { include 'b.php'; }
if (is_executable('c.php')) { include 'c.php'; }
