<?php
abstract class P { protected function tpl(): void {} abstract protected function x(): void; }
final class C extends P { protected function tpl(): void {} protected function x(): void {} }
