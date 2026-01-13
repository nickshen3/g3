#lang racket

;; ============================================
;; Example Racket file for g3 code search tests
;; ============================================

;; --- Basic Functions ---

(define (greet name)
  (printf "Hello, ~a!\n" name))

(define (add x y)
  (+ x y))

(define (factorial n)
  (if (<= n 1)
      1
      (* n (factorial (- n 1)))))

;; --- Variable Definitions ---

(define pi 3.14159)
(define greeting "Hello, World!")

;; --- Structs ---

(struct person (name age) #:transparent)
(struct point (x y) #:transparent)

(define (person-greet p)
  (printf "Hello, I'm ~a\n" (person-name p)))

;; --- Pattern Matching ---

(define (describe-list lst)
  (match lst
    ['() "empty"]
    [(list x) (format "singleton: ~a" x)]
    [(list x y) (format "pair: ~a, ~a" x y)]
    [_ "many elements"]))

(define (point-quadrant p)
  (match p
    [(point (? positive?) (? positive?)) 'first]
    [(point (? negative?) (? positive?)) 'second]
    [(point (? negative?) (? negative?)) 'third]
    [(point (? positive?) (? negative?)) 'fourth]
    [_ 'origin-or-axis]))

;; --- Lambda and Higher-Order Functions ---

(define double (lambda (x) (* x 2)))
(define triple (λ (x) (* x 3)))

(define (apply-twice f x)
  (f (f x)))

;; --- Let Bindings ---

(define (circle-area radius)
  (let ([pi 3.14159])
    (* pi radius radius)))

(define (swap-and-sum a b)
  (let* ([temp a]
         [a b]
         [b temp])
    (+ a b)))

;; --- For Loops ---

(define (sum-squares n)
  (for/sum ([i (in-range 1 (add1 n))])
    (* i i)))

(define (collect-evens n)
  (for/list ([i (in-range n)]
             #:when (even? i))
    i))

(define (matrix-coords rows cols)
  (for*/list ([r (in-range rows)]
              [c (in-range cols)])
    (cons r c)))

;; --- Macros ---

(define-syntax-rule (swap! x y)
  (let ([tmp x])
    (set! x y)
    (set! y tmp)))

(define-syntax-rule (unless condition body ...)
  (when (not condition)
    body ...))

;; --- Contracts ---

(define/contract (safe-divide x y)
  (-> number? (and/c number? (not/c zero?)) number?)
  (/ x y))

(define/contract (non-negative-add a b)
  (-> (>=/c 0) (>=/c 0) (>=/c 0))
  (+ a b))

;; --- Require and Provide ---

(require racket/string)
(require racket/list)

;; --- Module ---

(module+ test
  (require rackunit)
  
  (check-equal? (add 2 3) 5)
  (check-equal? (factorial 5) 120)
  (check-equal? (sum-squares 3) 14)
  (check-equal? (describe-list '()) "empty")
  (check-equal? (describe-list '(1)) "singleton: 1"))

(module+ main
  (greet "World")
  (displayln (add 5 3))
  (displayln (factorial 5))
  
  (define alice (person "Alice" 30))
  (person-greet alice)
  
  (displayln (sum-squares 10))
  (displayln (collect-evens 10)))
