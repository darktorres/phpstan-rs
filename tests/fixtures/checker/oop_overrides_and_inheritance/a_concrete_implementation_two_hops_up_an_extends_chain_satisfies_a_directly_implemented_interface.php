<?php
            interface Throwable2 {
                public function getMessage(): string;
                public function getCode();
            }
            class Exception2 implements Throwable2 {
                public function getMessage(): string {}
                public function getCode() {}
            }
            class BaseException2 extends Exception2 implements Throwable2 {}
            class Leaf2 extends BaseException2 implements Throwable2 {}
        