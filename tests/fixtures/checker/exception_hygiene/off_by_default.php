<?php
function f(): void { try { f(); } catch (\Throwable $e) {} }
